use crate::Error;
use crate::Message;
use crate::Socket;
use crate::{CloseCode, CloseFrame};
use async_trait::async_trait;
use std::fmt::Formatter;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[async_trait]
pub trait SessionExt: Send {
    /// Custom identification number of SessionExt, usually a number or a string.
    type ID: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display;
    /// Type the custom call - parameters passed to `on_call`.
    type Call: Send;

    /// Returns ID of the session.
    fn id(&self) -> &Self::ID;
    /// Handler for text messages from the client.
    async fn on_text(&mut self, text: String) -> Result<(), Error>;
    /// Handler for binary messages from the client.
    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program.
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error>;
}

pub struct Session<I, C> {
    pub id: I,
    socket: mpsc::UnboundedSender<Message>,
    calls: mpsc::UnboundedSender<C>,
    pub(crate) jh: Arc<std::sync::Mutex<Option<JoinHandle<Result<Option<CloseFrame>, Error>>>>>,
}

impl<I: std::fmt::Debug, C> std::fmt::Debug for Session<I, C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<I: Clone, C> Clone for Session<I, C> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            socket: self.socket.clone(),
            calls: self.calls.clone(),
            jh: self.jh.clone(),
        }
    }
}

impl<I: std::fmt::Display + Clone + Send, C: Send> Session<I, C> {
    pub fn create<S: SessionExt<ID = I, Call = C> + 'static>(
        session_fn: impl FnOnce(Session<I, C>) -> S,
        session_id: I,
        socket: Socket,
    ) -> Self {
        let (socket_sender, socket_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();

        let handle = Self {
            id: session_id.clone(),
            socket: socket_sender,
            calls: call_sender,
            jh: Arc::new(std::sync::Mutex::new(None)),
        };
        let session = session_fn(handle.clone());
        let actor = SessionActor {
            extension: session,
            id: session_id,
            socket_receiver,
            call_receiver,
            socket,
        };
        let jh = tokio::spawn(actor.run());
        *handle.jh.lock().unwrap() = Some(jh);
        handle
    }
}

impl<I: std::fmt::Display + Clone, C> Session<I, C> {
    /// Checks if the Session is still alive, if so you can proceed sending calls or messages.
    pub fn alive(&self) -> bool {
        !self.socket.is_closed() && !self.calls.is_closed()
    }

    /// Sends a Text message to the server
    pub fn text(&self, text: String) {
        self.socket
            .send(Message::Text(text))
            .unwrap_or_else(|_| tracing::warn!("Session::text {PANIC_MESSAGE_UNHANDLED_CLOSE}"));
    }

    /// Sends a Binary message to the server
    pub fn binary(&self, bytes: Vec<u8>) {
        self.socket
            .send(Message::Binary(bytes))
            .unwrap_or_else(|_| tracing::warn!("Session::binary {PANIC_MESSAGE_UNHANDLED_CLOSE}"));
    }

    /// Calls a method on the session
    pub fn call(&self, call: C) {
        self.calls
            .send(call)
            .unwrap_or_else(|_| tracing::warn!("Session::call {PANIC_MESSAGE_UNHANDLED_CLOSE}"));
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the call which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> C,
    ) -> Option<R> {
        let (sender, receiver) = oneshot::channel();
        let call = f(sender);

        self.calls.send(call).ok()?;
        receiver.await.ok()
    }
}

pub(crate) struct SessionActor<E: SessionExt> {
    pub extension: E,
    id: E::ID,
    socket_receiver: mpsc::UnboundedReceiver<Message>,
    call_receiver: mpsc::UnboundedReceiver<E::Call>,
    socket: Socket,
}

impl<E: SessionExt> SessionActor<E> {
    pub(crate) async fn run(mut self) -> Result<Option<CloseFrame>, Error> {
        let mut interval = tokio::time::interval(self.socket.config.heartbeat);
        let last_alive = Instant::now();
        loop {
            tokio::select! {
                biased;
                _tick = interval.tick() => {
                    if last_alive.elapsed() > self.socket.config.timeout {
                        tracing::info!("closing connection due to timeout");
                         self.socket.sink.send(Message::Close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: String::from("client didn't respond to Ping frame"),
                        })))?;
                        break;
                    }
                    // Use chrono Utc::now()
                    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    let timestamp = timestamp.as_millis();
                    let bytes = timestamp.to_be_bytes();
                    self.socket.sink.send(Message::Ping(bytes.to_vec()))?;
                }
                Some(message) = self.socket_receiver.recv() => {
                    self.socket.sink.send(message.clone())?;
                    if let Message::Close(frame) = message {
                        return Ok(frame)
                    }
                }
                Some(call) = self.call_receiver.recv() => {
                    self.extension.on_call(call).await?;
                }
                message = self.socket.stream.recv() => {
                    match message {
                        Some(Ok(message)) => match message {
                            Message::Text(text) => self.extension.on_text(text).await?,
                            Message::Binary(bytes) => self.extension.on_binary(bytes).await?,
                            Message::Close(frame) => {
                                return Ok(frame.map(CloseFrame::from))
                            },
                            _ => {}
                        }
                        Some(Err(error)) => {
                            tracing::error!(id = %self.id, "connection error: {error}");
                            return Err(error.into())
                        }
                        None => break
                    };
                }
                else => break,
            }
        }
        Ok(None)
    }
}

const PANIC_MESSAGE_UNHANDLED_CLOSE: &str = "should not be called after Session close. Try handling Server::disconnect or Session::drop, also you can check whether the Session is alive using Session::alive";
