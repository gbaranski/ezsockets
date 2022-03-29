use std::sync::Arc;

use crate::CloseFrame;
use crate::Error;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

#[async_trait]
pub trait SessionExt: Send {
    type ID: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display;
    /// Arguments passed for creating a new session on server.
    type Args: std::fmt::Debug + Send;
    type Params: std::fmt::Debug + Send;

    fn id(&self) -> &Self::ID;
    async fn text(&mut self, text: String) -> Result<(), Error>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    async fn call(&mut self, params: Self::Params) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct Session<I: std::fmt::Display + Clone, P: std::fmt::Debug> {
    pub id: I,
    socket: mpsc::UnboundedSender<Message>,
    calls: mpsc::UnboundedSender<P>,
    closed: Arc<Mutex<Option<oneshot::Receiver<Result<Option<CloseFrame>, Error>>>>>,
}

impl<I: std::fmt::Display + Clone, P: std::fmt::Debug> std::clone::Clone for Session<I, P> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            socket: self.socket.clone(),
            calls: self.calls.clone(),
            closed: self.closed.clone(),
        }
    }
}

impl<I: std::fmt::Display + Clone + Send, P: std::fmt::Debug + Send> Session<I, P> {
    pub fn create<S: SessionExt<ID = I, Params = P> + 'static>(
        session_fn: impl FnOnce(Session<I, P>) -> S,
        session_id: I,
        socket: Socket,
    ) -> Self {
        let (socket_sender, socket_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();
        let (closed_sender, closed_receiver) = oneshot::channel();
        let handle = Self {
            id: session_id.clone(),
            socket: socket_sender,
            calls: call_sender,
            closed: Arc::new(Mutex::new(Some(closed_receiver))),
        };
        let session = session_fn(handle.clone());
        let mut actor =
            SessionActor::new(session, session_id, socket_receiver, call_receiver, socket);

        tokio::spawn(async move {
            let result = actor.run().await;
            closed_sender.send(result).unwrap();
        });

        handle
    }
}

impl<I: std::fmt::Display + Clone, P: std::fmt::Debug> Session<I, P> {
    #[doc(hidden)]
    /// WARN: Use only if really nessesary.
    /// 
    /// this uses some hack, which takes ownership of underlaying `oneshot::Receiver`, making it unaccessible for all future calls of this method. 
    pub(super) async fn closed(&self) -> Result<Option<CloseFrame>, Error> {
        let mut closed = self.closed.lock().await;
        let closed = closed
            .take()
            .expect("someone already called .closed() before");
        closed.await.unwrap()
    }

    /// Sends a Text message to the server
    pub async fn text(&self, text: String) {
        self.socket.send(Message::Text(text)).unwrap();
    }

    /// Sends a Binary message to the server
    pub async fn binary(&self, bytes: Vec<u8>) {
        self.socket.send(Message::Binary(bytes)).unwrap();
    }

    /// Calls a method on the session
    pub async fn call(&self, params: P) {
        self.calls.send(params).unwrap();
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the Params which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> P,
    ) -> R {
        let (sender, receiver) = oneshot::channel();
        let params = f(sender);

        self.calls.send(params).unwrap();
        let response = receiver.await.unwrap();

        response
    }
}

pub(crate) struct SessionActor<E: SessionExt> {
    pub extension: E,
    id: E::ID,
    socket_receiver: mpsc::UnboundedReceiver<Message>,
    call_receiver: mpsc::UnboundedReceiver<E::Params>,
    socket: Socket,
}

impl<E: SessionExt> SessionActor<E> {
    pub(crate) fn new(
        extension: E,
        id: E::ID,
        socket_receiver: mpsc::UnboundedReceiver<Message>,
        call_receiver: mpsc::UnboundedReceiver<E::Params>,
        socket: Socket,
    ) -> Self {
        Self {
            id,
            extension,
            socket_receiver,
            call_receiver,
            socket,
        }
    }

    pub(crate) async fn run(&mut self) -> Result<Option<CloseFrame>, Error> {
        loop {
            tokio::select! {
                Some(message) = self.socket_receiver.recv() => {
                    self.socket.send(message.clone().into()).await;
                    if let Message::Close(frame) = message {
                        return Ok(frame)
                    }
                }
                Some(params) = self.call_receiver.recv() => {
                    self.extension.call(params).await?;
                }
                message = self.socket.recv() => {
                    match message {
                        Some(Ok(message)) => match message {
                            Message::Text(text) => self.extension.text(text).await?,
                            Message::Binary(bytes) => self.extension.binary(bytes).await?,
                            Message::Close(frame) => {
                                return Ok(frame.map(CloseFrame::from))
                            },
                        }
                        Some(Err(error)) => {
                            tracing::error!(id = %self.id, "connection error: {error}");

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
