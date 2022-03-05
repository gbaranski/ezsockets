use crate::BoxError;
use crate::CloseFrame;
use crate::Message;
use crate::WebSocketStream;
use async_trait::async_trait;
use futures::SinkExt;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;

#[async_trait]
pub trait Session: Send {
    type ID: Clone + std::fmt::Debug + std::fmt::Display;

    fn id(&self) -> &Self::ID;
    async fn text(&mut self, text: String) -> Result<Option<Message>, BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<Message>, BoxError>;
    async fn disconnected(&mut self) -> Result<(), BoxError>;
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    sender: mpsc::UnboundedSender<Message>,
}

impl SessionHandle {
    pub fn new(sender: mpsc::UnboundedSender<Message>) -> Self {
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
    receiver: mpsc::UnboundedReceiver<Message>,
    stream: WebSocketStream,
    heartbeat: Instant,
}

impl<E: Session> SessionActor<E> {
    pub(crate) fn new(
        extension: E,
        receiver: mpsc::UnboundedReceiver<Message>,
        stream: WebSocketStream,
        heartbeat: Instant,
    ) -> Self {
        Self {
            extension,
            receiver,
            stream,
            heartbeat,
        }
    }

    pub(crate) async fn run(&mut self) -> Result<Option<CloseFrame>, BoxError> {
        use futures::StreamExt;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    let message = match message {
                        Message::Text(text) => tungstenite::Message::Text(text),
                        Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
                        Message::Close(frame) => {
                            return Ok(frame);
                        }
                    };
                    self.stream.send(message).await?;
                }
                Some(message) = self.stream.next() => {
                    let message = message?;
                    let message = match message {
                        tungstenite::Message::Text(text) => self.extension.text(text).await?,
                        tungstenite::Message::Binary(bytes) => self.extension.binary(bytes).await?,
                        tungstenite::Message::Ping(bytes) => {
                            self.stream.send(tungstenite::Message::Pong(bytes)).await?;
                            None
                        }
                        tungstenite::Message::Pong(_) => {
                            // TODO: Maybe handle bytes?
                            self.heartbeat = Instant::now();
                            None
                        }
                        tungstenite::Message::Close(frame) => {
                            return Ok(frame.map(CloseFrame::from))
                        }
                        tungstenite::Message::Frame(_) => todo!(),
                    };
                    if let Some(message) = message {
                        self.stream.send(message.into()).await?;
                    }
                }
                _ = interval.tick() => {
                    self.stream.send(tungstenite::Message::Ping(Vec::new())).await?;
                }
                else => break,
            }
        }

        Ok(None)
    }
}
