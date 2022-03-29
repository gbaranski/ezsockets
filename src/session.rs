use crate::Error;
use crate::CloseFrame;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[async_trait]
pub trait SessionExt: Send {
    type ID: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display;
    type Params: std::fmt::Debug;

    fn id(&self) -> &Self::ID;
    async fn text(&mut self, text: String) -> Result<(), Error>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    async fn call(&mut self, params: Self::Params) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct Session<P: std::fmt::Debug = ()> {
    socket: mpsc::UnboundedSender<Message>,
    calls: mpsc::UnboundedSender<P>,
}

impl<P: std::fmt::Debug> std::clone::Clone for Session<P> {
    fn clone(&self) -> Self {
        Self {
            socket: self.socket.clone(),
            calls: self.calls.clone(),
        }
    }
}

impl<P: std::fmt::Debug + Send> Session<P> {
    pub fn create<S: SessionExt<Params = P> + 'static>(
        session_fn: impl FnOnce(Session<P>) -> S,
        socket: Socket,
    ) -> Self {
        let (socket_sender, socket_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();
        let handle = Self {
            socket: socket_sender,
            calls: call_sender,
        };
        let session = session_fn(handle.clone());
        let id = session.id().to_owned();
        let mut actor = SessionActor::new(session, id, socket_receiver, call_receiver, socket);
        tokio::spawn(async move { actor.run().await });

        handle
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
    pub async fn call_with<R: std::fmt::Debug>(&self, f: impl FnOnce(oneshot::Sender<R>) -> P) -> R {
        let (sender, receiver) = oneshot::channel();
        let params = f(sender);

        self.calls.send(params).unwrap();
        let response = receiver.await.unwrap();

        response
    }
}

pub(crate) struct SessionActor<E: SessionExt> {
    pub extension: E,
    pub id: E::ID,
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
            extension,
            id,
            socket_receiver,
            call_receiver,
            socket,
        }
    }

    pub(crate) async fn run(&mut self) {
        let id = self.id.to_owned();
        let result: Result<_, Error> = async move {
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
                            Some(message) => match message {
                                Message::Text(text) => self.extension.text(text).await?,
                                Message::Binary(bytes) => self.extension.binary(bytes).await?,
                                Message::Close(frame) => {
                                    return Ok(frame.map(CloseFrame::from))
                                },
                            }
                            None => break
                        };
                    }
                    else => break,
                }
            }
            Ok(None)
        }
        .await;

        match result {
            Ok(Some(CloseFrame { code, reason })) => {
                tracing::info!(%id, ?code, %reason, "connection closed")
            }
            Ok(None) => tracing::info!(%id, "connection closed"),
            Err(err) => tracing::warn!(%id, "connection error: {err}"),
        };
    }
}
