//! # WebSocket Server
//! ## Get started
//!
//! To create a simple echo server, we need to define a `Session` struct.
//! The code below represents a simple echo server.
//!
//! ```
//! use async_trait::async_trait;
//!
//! // Create our own session that implements `SessionExt`
//!
//! type SessionID = u16;
//! type Session = ezsockets::Session<SessionID, ()>;
//!
//! struct EchoSession {
//!     handle: Session,
//!     id: SessionID,
//! }
//!
//! #[async_trait]
//! impl ezsockets::SessionExt for EchoSession {
//!     type ID = SessionID;
//!     type Call = ();
//!
//!     fn id(&self) -> &Self::ID {
//!         &self.id
//!     }
//!
//!     // Define handlers for incoming messages
//!
//!     async fn on_text(&mut self, text: ezsockets::Utf8Bytes) -> Result<(), ezsockets::Error> {
//!         self.handle.text(text); // Send response to the client
//!         Ok(())
//!     }
//!
//!     async fn on_binary(&mut self, _bytes: ezsockets::Bytes) -> Result<(), ezsockets::Error> {
//!         unimplemented!()
//!     }
//!
//!     // `on_call` is for custom messages, it's useful if you want to send messages from other thread or tokio task. Use `Session::call` to send a message.
//!     async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> {
//!         let () = call;
//!         Ok(())
//!     }
//! }
//!
//! // Create our own server that implements `ServerExt`
//!
//! use ezsockets::Server;
//! use std::net::SocketAddr;
//!
//! struct EchoServer {}
//!
//! #[async_trait]
//! impl ezsockets::ServerExt for EchoServer {
//!     type Session = EchoSession;
//!     type Call = ();
//!
//!     async fn on_connect(
//!         &mut self,
//!         socket: ezsockets::Socket,
//!         request: ezsockets::Request,
//!         address: SocketAddr,
//!     ) -> Result<Session, Option<ezsockets::CloseFrame>> {
//!         let id = address.port();
//!         let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
//!         Ok(session)
//!     }
//!
//!     async fn on_disconnect(
//!         &mut self,
//!         _id: <Self::Session as ezsockets::SessionExt>::ID,
//!         _reason: Result<Option<ezsockets::CloseFrame>, ezsockets::Error>
//!     ) -> Result<(), ezsockets::Error> {
//!         Ok(())
//!     }
//!
//!     async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> {
//!         let () = call;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! That's it! To start the server you need to one of supported backends:
//! - [tungstenite](crate::tungstenite) - the simplest option based on [`tokio-tungstenite`](https://crates.io/crates/tokio-tungstenite)
//! - [axum](crate::axum) - based on [`axum`](https://crates.io/crates/axum), a web framework for Rust

use crate::socket::CloseCode;
use crate::socket::InMessage;
use crate::CloseFrame;
use crate::Error;
use crate::Message;
use crate::Request;
use crate::Session;
use crate::SessionExt;
use crate::Socket;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinHandle;

struct NewConnection {
    socket: Socket,
    address: SocketAddr,
    request: Request,
}

struct Disconnected<E: ServerExt> {
    id: <E::Session as SessionExt>::ID,
    result: Result<Option<CloseFrame>, Error>,
}

struct ServerActor<E: ServerExt> {
    connection_receiver: mpsc::UnboundedReceiver<NewConnection>,
    disconnection_receiver: mpsc::UnboundedReceiver<Disconnected<E>>,
    server_call_receiver: mpsc::UnboundedReceiver<E::Call>,
    shutdown_receiver: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    shutdown_signal: watch::Sender<bool>,
    server: Server<E>,
    extension: E,
}

impl<E: ServerExt> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as SessionExt>::ID: Send,
{
    async fn run(mut self) {
        tracing::info!("starting websocket server");
        let mut active_sessions: usize = 0;
        let mut shutting_down = false;
        let mut shutdown_acks: Vec<oneshot::Sender<()>> = Vec::new();
        loop {
            if shutting_down && active_sessions == 0 {
                tracing::info!("server shutdown complete");
                for ack in shutdown_acks.drain(..) {
                    let _ = ack.send(());
                }
                break;
            }
            if let Err(err) = async {
                tokio::select! {
                    Some(ack) = self.shutdown_receiver.recv() => {
                        shutdown_acks.push(ack);
                        if !shutting_down {
                            tracing::info!(active_sessions, "graceful shutdown initiated");
                            shutting_down = true;
                            // Broadcast to per-session watchers; they will send a
                            // Close frame to each live session. New connections
                            // and on_call dispatches are halted below by the
                            // `if !shutting_down` guards.
                            let _ = self.shutdown_signal.send(true);
                            if active_sessions == 0 {
                                // No live sessions — we'll exit on the next loop
                                // header check after acks drain into the Vec.
                            }
                        }
                    }
                    Some(NewConnection{socket, address, request}) = self.connection_receiver.recv() => {
                        if shutting_down {
                            // Reject connections queued after shutdown began rather
                            // than leaving them hanging on a never-drained channel.
                            tracing::info!("rejecting connection from {address} during shutdown");
                            let _ = socket
                                .sink
                                .send(InMessage::new(Message::Close(Some(CloseFrame {
                                    code: CloseCode::Away,
                                    reason: "server shutting down".into(),
                                }))))
                                .await;
                            return Ok(());
                        }
                        let socket_sink = socket.sink.clone();
                        match self.extension.on_connect(socket, request, address).await {
                            Ok(session) => {
                                tracing::info!("connection from {address} accepted");
                                let session_id = session.id.clone();
                                active_sessions += 1;

                                let shutdown_rx = self.shutdown_signal.subscribe();
                                tokio::spawn({
                                    let server = self.server.clone();
                                    let close_session = session.clone();
                                    async move {
                                        let close_task = tokio::spawn(watch_for_shutdown(shutdown_rx, close_session));
                                        let result = session.await_close().await;
                                        close_task.abort();
                                        server.disconnected(session_id, result);
                                    }
                                });
                            }
                            Err(err) => {
                                // our extension rejected the connection, so forward the close frame to the client
                                tracing::info!(?err, "connection from {address} rejected");
                                if let Err(err) = socket_sink.send(InMessage::new(Message::Close(err))).await {
                                    tracing::warn!(?err, "failed forwarding close frame to socket after connection rejected");
                                }
                            }
                        }
                    }
                    Some(Disconnected{id, result}) = self.disconnection_receiver.recv() => {
                        match &result {
                            Ok(Some(CloseFrame { code, reason })) => {
                                tracing::info!(%id, ?code, %reason, "connection closed")
                            }
                            Ok(None) => tracing::info!(%id, "connection closed"),
                            Err(err) => tracing::warn!(%id, "connection closed due to: {err:?}"),
                        };
                        active_sessions = active_sessions.saturating_sub(1);
                        self.extension.on_disconnect(id.clone(), result).await?;
                    }
                    Some(call) = self.server_call_receiver.recv(), if !shutting_down => {
                        self.extension.on_call(call).await?
                    }
                    else => return Err("server actor branches broken".into()),
                }
                Ok::<_, Error>(())
            }
                .await {
                tracing::warn!("error when processing: {err:?}");
            }
        }
    }
}

async fn watch_for_shutdown<I, C>(mut rx: watch::Receiver<bool>, session: Session<I, C>)
where
    I: std::fmt::Display + Clone + Send + 'static,
    C: Send + 'static,
{
    // Wait until the value flips to `true`. If all senders drop, exit quietly.
    loop {
        if *rx.borrow() {
            break;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
    let _ = session.close(Some(CloseFrame {
        code: CloseCode::Away,
        reason: "server shutting down".into(),
    }));
}

#[async_trait]
pub trait ServerExt: Send {
    /// Type of the session that will be created for each connection.
    type Session: SessionExt;
    /// Type the custom call - parameters passed to `on_call`.
    type Call: Send;

    /// Called when client connects to the server.
    ///
    /// Here you should create a `Session` with your own implementation of `SessionExt` and return it.
    /// If you don't want to accept the connection, return an error.
    async fn on_connect(
        &mut self,
        socket: Socket,
        request: Request,
        address: SocketAddr,
    ) -> Result<
        Session<<Self::Session as SessionExt>::ID, <Self::Session as SessionExt>::Call>,
        Option<CloseFrame>,
    >;
    /// Called when client disconnects from the server.
    async fn on_disconnect(
        &mut self,
        id: <Self::Session as SessionExt>::ID,
        reason: Result<Option<CloseFrame>, Error>,
    ) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program.
    ///
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct Server<E: ServerExt> {
    connection_sender: mpsc::UnboundedSender<NewConnection>,
    disconnection_sender: mpsc::UnboundedSender<Disconnected<E>>,
    server_call_sender: mpsc::UnboundedSender<E::Call>,
    shutdown_sender: mpsc::UnboundedSender<oneshot::Sender<()>>,
}

impl<E: ServerExt> From<Server<E>> for mpsc::UnboundedSender<E::Call> {
    fn from(server: Server<E>) -> Self {
        server.server_call_sender
    }
}

impl<E: ServerExt + 'static> Server<E> {
    pub fn create(create: impl FnOnce(Self) -> E) -> (Self, JoinHandle<()>) {
        let (connection_sender, connection_receiver) = mpsc::unbounded_channel();
        let (disconnection_sender, disconnection_receiver) = mpsc::unbounded_channel();
        let (server_call_sender, server_call_receiver) = mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_receiver) = mpsc::unbounded_channel();
        let (shutdown_signal, _) = watch::channel(false);
        let handle = Self {
            connection_sender,
            server_call_sender,
            disconnection_sender,
            shutdown_sender,
        };
        let extension = create(handle.clone());
        let actor = ServerActor {
            connection_receiver,
            disconnection_receiver,
            server_call_receiver,
            shutdown_receiver,
            shutdown_signal,
            extension,
            server: handle.clone(),
        };
        let future = tokio::spawn(actor.run());

        (handle, future)
    }
}

impl<E: ServerExt> Server<E> {
    /// Accept a connection. Logs an error if the server actor is dead.
    pub fn accept(&self, socket: Socket, request: Request, address: SocketAddr) {
        // TODO: can we refuse the connection here?
        if self
            .connection_sender
            .send(NewConnection {
                socket,
                request,
                address,
            })
            .is_err()
        {
            tracing::error!("accepted a connection but the server actor is dead");
        }
    }

    pub(crate) fn disconnected(
        &self,
        id: <E::Session as SessionExt>::ID,
        result: Result<Option<CloseFrame>, Error>,
    ) {
        self.disconnection_sender
            .send(Disconnected { id, result })
            .map_err(|_| ())
            .unwrap_or_default();
    }

    pub fn call(&self, call: E::Call) -> Result<(), mpsc::error::SendError<E::Call>> {
        self.server_call_sender.send(call)
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the call which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> E::Call,
    ) -> Option<R> {
        let (sender, receiver) = oneshot::channel();
        let call = f(sender);

        let Ok(_) = self.server_call_sender.send(call) else {
            return None;
        };
        let Ok(result) = receiver.await else {
            return None;
        };
        Some(result)
    }

    /// Initiates a graceful shutdown of the server and resolves once every
    /// session has been closed and dispatched to [`ServerExt::on_disconnect`].
    ///
    /// Behavior:
    /// - Stops accepting new connections delivered through [`Server::accept`];
    ///   incoming `NewConnection`s queued before the shutdown remain queued
    ///   but are not handed to [`ServerExt::on_connect`].
    /// - Stops dispatching [`Server::call`] messages to [`ServerExt::on_call`];
    ///   queued calls are ignored.
    /// - Sends a `Close` frame (code `Away`, reason `"server shutting down"`)
    ///   to every live session and waits for each session actor to terminate.
    /// - Awaits [`ServerExt::on_disconnect`] for every session that was live
    ///   when shutdown started, in the order the disconnects are received.
    /// - After this future resolves, the server-actor task returned by
    ///   [`Server::create`] also completes.
    ///
    /// Subsequent calls after the actor has exited return
    /// [`GracefulShutdownError::ServerStopped`].
    ///
    /// Calling this method concurrently from multiple tasks is safe — every
    /// caller is acknowledged once the drain finishes.
    pub async fn graceful_shutdown(&self) -> Result<(), GracefulShutdownError> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.shutdown_sender
            .send(ack_tx)
            .map_err(|_| GracefulShutdownError::ServerStopped)?;
        ack_rx.await.map_err(|_| GracefulShutdownError::ServerStopped)
    }
}

/// Errors returned by [`Server::graceful_shutdown`].
#[derive(Debug)]
pub enum GracefulShutdownError {
    /// The server actor has already exited (either because shutdown completed
    /// previously, or because the actor task was aborted).
    ServerStopped,
}

impl std::fmt::Display for GracefulShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServerStopped => write!(f, "server actor has already stopped"),
        }
    }
}

impl std::error::Error for GracefulShutdownError {}

impl<E: ServerExt> std::clone::Clone for Server<E> {
    fn clone(&self) -> Self {
        Self {
            connection_sender: self.connection_sender.clone(),
            disconnection_sender: self.disconnection_sender.clone(),
            server_call_sender: self.server_call_sender.clone(),
            shutdown_sender: self.shutdown_sender.clone(),
        }
    }
}
