use crate::BoxError;
use crate::CloseFrame;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait SessionExt: Send {
    type ID: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display;

    fn id(&self) -> &Self::ID;
    async fn text(&mut self, text: String) -> Result<(), BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), BoxError>;
}

#[derive(Debug, Clone)]
pub struct Session {
    sender: mpsc::UnboundedSender<Message>,
}

impl Session {
    pub fn create<S: SessionExt + 'static>(
        session_fn: impl FnOnce(Session) -> S,
        socket: Socket,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let handle = Self { sender };
        let session = session_fn(handle.clone());
        let id = session.id().to_owned();
        let mut actor = SessionActor::new(session, id, receiver, socket);
        tokio::spawn(async move { actor.run().await });

        handle
    }

    /// Sends a Text message to the server
    pub async fn text(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }

    /// Sends a Binary message to the server
    pub async fn binary(&self, bytes: Vec<u8>) {
        self.sender.send(Message::Binary(bytes)).unwrap();
    }
}

pub(crate) struct SessionActor<E: SessionExt> {
    pub extension: E,
    pub id: E::ID,
    receiver: mpsc::UnboundedReceiver<Message>,
    socket: Socket,
}

impl<E: SessionExt> SessionActor<E> {
    pub(crate) fn new(
        extension: E,
        id: E::ID,
        receiver: mpsc::UnboundedReceiver<Message>,
        socket: Socket,
    ) -> Self {
        Self {
            extension,
            id,
            receiver,
            socket,
        }
    }

    pub(crate) async fn run(&mut self) {
        let id = self.id.to_owned();
        let result: Result<_, BoxError> = async move {
            loop {
                tokio::select! {
                    Some(message) = self.receiver.recv() => {
                        self.socket.send(message.clone().into()).await;
                        if let Message::Close(frame) = message {
                            return Ok(frame)
                        }
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
