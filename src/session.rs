use crate::BoxError;
use crate::CloseFrame;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait Session: Send {
    type ID: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display;

    fn id(&self) -> &Self::ID;
    async fn text(&mut self, text: String) -> Result<Option<Message>, BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<Message>, BoxError>;
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    sender: mpsc::UnboundedSender<Message>,
}

impl SessionHandle {
    pub fn create<S: Session + 'static>(session: S, socket: Socket) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let id = session.id().to_owned();
        let mut actor = SessionActor::new(session, id, receiver, socket);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn text(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }

    pub async fn binary(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }
}

pub(crate) struct SessionActor<E: Session> {
    pub extension: E,
    pub id: E::ID,
    receiver: mpsc::UnboundedReceiver<Message>,
    socket: Socket,
}

impl<E: Session> SessionActor<E> {
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
                    Some(message) = self.socket.recv() => {
                        let message = match message {
                            Message::Text(text) => self.extension.text(text).await?,
                            Message::Binary(bytes) => self.extension.binary(bytes).await?,
                            Message::Close(frame) => {
                                return Ok(frame.map(CloseFrame::from))
                            },

                        };
                        if let Some(message) = message {
                            self.socket.send(message.into()).await;
                        }
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
