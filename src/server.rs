//! ## Get started
//!
//! To create a simple echo server, we need to define a `Session` struct.
//! The code below represents a simple echo server.
//!
//! ```ignore
//! use async_trait::async_trait;
//! use ezsockets::Session;
//!
//! type SessionID = u16;
//!
//! struct EchoSession {
//!     handle: Session,
//!     id: SessionID,
//! }
//!
//! #[async_trait]
//! impl ezsockets::SessionExt for EchoSession {
//!     type ID = SessionID;
//!     type Args = ();
//!     type Params = ();
//!
//!     fn id(&self) -> &Self::ID {
//!         &self.id
//!     }
//!
//!     async fn text(&mut self, text: String) -> Result<(), ezsockets::Error> {
//!         self.handle.text(text); // Send response to the client
//!         Ok(())
//!     }
//!
//!     async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
//!         unimplemented!()
//!     }
//!
//!     async fn call(&mut self, params: Self::Params) -> Result<(), ezsockets::Error> {
//!         let () = params;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Then, we need to define a `Server` struct
//!
//!
//! ```ignore
//! use async_trait::async_trait;
//! use ezsockets::Server;
//! use ezsockets::Session;
//! use ezsockets::Socket;
//! use std::net::SocketAddr;
//!
//! struct EchoServer {}
//!
//! #[async_trait]
//! impl ezsockets::ServerExt for EchoServer {
//!     type Session = EchoSession;
//!     type Params = ();
//!
//!     async fn accept(
//!         &mut self,
//!         socket: Socket,
//!         address: SocketAddr,
//!         _args: (),
//!     ) -> Result<Session, ezsockets::Error> {
//!         let id = address.port();
//!         let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
//!         Ok(session)
//!     }
//!
//!     async fn disconnected(
//!         &mut self,
//!         _id: <Self::Session as ezsockets::SessionExt>::ID,
//!     ) -> Result<(), ezsockets::Error> {
//!         Ok(())
//!     }
//!
//!     async fn call(&mut self, params: Self::Params) -> Result<(), ezsockets::Error> {
//!         let () = params;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! That's it! To start the server you need to one of supported backends:
//! - [tungstenite](crate::tungstenite) - the simplest option based on [`tokio-tungstenite`](https://crates.io/crates/tokio-tungstenite)
//! - [axum](crate::axum) - based on [`axum`](https://crates.io/crates/axum), a web framework for Rust

use crate::CloseFrame;
use crate::Error;
use crate::Session;
use crate::SessionExt;
use crate::Socket;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

struct NewConnection<E: ServerExt> {
    socket: Socket,
    address: SocketAddr,
    args: <E::Session as SessionExt>::Args,
    respond_to: oneshot::Sender<<E::Session as SessionExt>::ID>,
}

struct Disconnected<E: ServerExt> {
    id: <E::Session as SessionExt>::ID,
    result: Result<Option<CloseFrame>, Error>,
}

struct ServerActor<E: ServerExt> {
    connections: mpsc::UnboundedReceiver<NewConnection<E>>,
    disconnections: mpsc::UnboundedReceiver<Disconnected<E>>,
    calls: mpsc::UnboundedReceiver<E::Call>,
    server: Server<E>,
    extension: E,
}

impl<E: ServerExt> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as SessionExt>::ID: Send,
{
    async fn run(&mut self) -> Result<(), Error> {
        tracing::info!("starting server");
        loop {
            tokio::select! {
                Some(NewConnection{socket, address, args, respond_to}) = self.connections.recv() => {
                    let session = self.extension.on_connect(socket, address, args).await?;
                    let session_id = session.id.clone();
                    tracing::info!("connection from {address} accepted");
                    respond_to.send(session_id.clone()).unwrap();

                    tokio::spawn({
                        let server = self.server.clone();
                        async move {
                            let result = session.closed().await;
                            server.disconnected(session_id, result).await;
                        }
                    });
                }
                Some(Disconnected{id, result}) = self.disconnections.recv() => {
                    self.extension.on_disconnect(id.clone()).await?;
                    match result {
                        Ok(Some(CloseFrame { code, reason })) => {
                            tracing::info!(%id, ?code, %reason, "connection closed")
                        }
                        Ok(None) => tracing::info!(%id, "connection closed"),
                        Err(err) => tracing::warn!(%id, "connection closed due to: {err}"),
                    };
                }
                Some(call) = self.calls.recv() => {
                    self.extension.on_call(call).await?
                }
                else => break
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait ServerExt: Send {
    /// Type of the session that will be created for each connection.
    type Session: SessionExt;
    /// Type the custom call - parameters passed to `on_call`.
    type Call: Send + std::fmt::Debug;

    /// Called when client connects to the server.
    /// Here you should create a `Session` with your own implementation of `SessionExt` and return it.
    ///If you don't want to accept the connection, return an error.
    async fn on_connect(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        args: <Self::Session as SessionExt>::Args,
    ) -> Result<
        Session<<Self::Session as SessionExt>::ID, <Self::Session as SessionExt>::Call>,
        Error,
    >;
    /// Called when client disconnects from the server.
    async fn on_disconnect(&mut self, id: <Self::Session as SessionExt>::ID) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program.
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct Server<E: ServerExt> {
    connections: mpsc::UnboundedSender<NewConnection<E>>,
    disconnections: mpsc::UnboundedSender<Disconnected<E>>,
    calls: mpsc::UnboundedSender<E::Call>,
}

impl<E: ServerExt> From<Server<E>> for mpsc::UnboundedSender<E::Call> {
    fn from(server: Server<E>) -> Self {
        server.calls
    }
}

impl<E: ServerExt + 'static> Server<E> {
    pub fn create(
        create: impl FnOnce(Self) -> E,
    ) -> (Self, impl Future<Output = Result<(), Error>>) {
        let (connection_sender, connection_receiver) = mpsc::unbounded_channel();
        let (disconnection_sender, disconnection_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();
        let handle = Self {
            connections: connection_sender,
            calls: call_sender,
            disconnections: disconnection_sender,
        };
        let extension = create(handle.clone());
        let mut actor = ServerActor {
            connections: connection_receiver,
            disconnections: disconnection_receiver,
            calls: call_receiver,
            extension,
            server: handle.clone(),
        };
        let future = tokio::spawn(async move {
            actor.run().await?;
            Ok::<_, Error>(())
        });
        let future = async move { future.await.unwrap() };
        (handle, future)
    }
}

impl<E: ServerExt> Server<E> {
    pub(crate) async fn accept(
        &self,
        socket: Socket,
        address: SocketAddr,
        args: <E::Session as SessionExt>::Args,
    ) -> <E::Session as SessionExt>::ID {
        let (sender, receiver) = oneshot::channel();
        self.connections
            .send(NewConnection {
                socket,
                address,
                args,
                respond_to: sender,
            })
            .map_err(|_| ())
            .unwrap();
        receiver.await.unwrap()
    }

    pub(crate) async fn disconnected(
        &self,
        id: <E::Session as SessionExt>::ID,
        result: Result<Option<CloseFrame>, Error>,
    ) {
        self.disconnections
            .send(Disconnected { id, result })
            .map_err(|_| ())
            .unwrap();
    }

    pub fn call(&self, params: E::Call) {
        self.calls.send(params).map_err(|_| ()).unwrap();
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the Params which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> E::Call,
    ) -> R {
        let (sender, receiver) = oneshot::channel();
        let params = f(sender);

        self.calls.send(params).unwrap();
        receiver.await.unwrap()
    }
}

impl<E: ServerExt> std::clone::Clone for Server<E> {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            disconnections: self.disconnections.clone(),
            calls: self.calls.clone(),
        }
    }
}
