use crate::BoxError;
use crate::CloseCode;
use crate::Message;
use async_trait::async_trait;
use futures::SinkExt;
use std::future::Future;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use url::Url;

type WebSocketStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[async_trait]
pub trait Client: Send {
    async fn text(&mut self, text: String) -> Result<Option<Message>, BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<Message>, BoxError>;
    async fn closed(
        &mut self,
        code: Option<CloseCode>,
        reason: Option<String>,
    ) -> Result<(), BoxError>;
}

#[derive(Debug, Clone)]
pub struct ClientHandle {
    sender: mpsc::UnboundedSender<Message>,
}

impl ClientHandle {
    pub async fn text(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }

    pub async fn binary(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }
}

pub async fn connect<C: Client + 'static>(
    client: C,
    url: Url,
) -> (ClientHandle, impl Future<Output = Result<(), BoxError>>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = ClientHandle { sender };
    let future = tokio::spawn(async move {
        let (stream, _) = tokio_tungstenite::connect_async(url).await?;
        let actor = ClientActor {
            client,
            receiver,
            stream,
            heartbeat: Instant::now(),
        };
        actor.run().await?;
        Ok::<_, BoxError>(())
    });
    let future = async move { future.await.unwrap() };
    (handle, future)
}

struct ClientActor<C: Client> {
    client: C,
    receiver: mpsc::UnboundedReceiver<Message>,
    stream: WebSocketStream,
    heartbeat: Instant,
}

impl<C: Client> ClientActor<C> {
    async fn run(mut self) -> Result<(), BoxError> {
        use futures::StreamExt;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    let should_continue = self.handle_message(message).await?;
                    if !should_continue {
                        return Ok(())
                    }
                }
                Some(message) = self.stream.next() => {
                    let message = message?;
                    let should_continue = self.handle_websocket_message(message).await?;
                    if !should_continue {
                        return Ok(())
                    }
                }
                _ = interval.tick() => {
                    self.stream.send(tungstenite::Message::Ping(Vec::new())).await?;
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> Result<bool, BoxError> {
        let message = match message {
            Message::Text(text) => tungstenite::Message::Text(text),
            Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
            Message::Close(frame) => {
                let frame = frame.map(|(code, reason)| tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                });
                self.stream.close(frame).await?;
                return Ok(false);
            }
        };
        self.stream.send(message).await?;
        Ok(true)
    }

    async fn handle_websocket_message(
        &mut self,
        message: tungstenite::Message,
    ) -> Result<bool, BoxError> {
        let message = match message {
            tungstenite::Message::Text(text) => self.client.text(text).await?,
            tungstenite::Message::Binary(bytes) => self.client.binary(bytes).await?,
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
                let (code, reason) = if let Some(frame) = frame {
                    (Some(frame.code.into()), Some(frame.reason.into()))
                } else {
                    (None, None)
                };
                self.client.closed(code, reason).await?;
                return Ok(false);
            }
            tungstenite::Message::Frame(_) => todo!(),
        };
        if let Some(message) = message {
            self.stream.send(message.into()).await?;
        }
        Ok(true)
    }
}
