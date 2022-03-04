use crate::BoxError;
use crate::CloseCode;
use crate::Message;
use async_trait::async_trait;
use futures::SinkExt;
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use url::Url;

const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::new(5, 0);

#[derive(Debug)]
pub struct ClientConfig {
    pub url: Url,
    pub reconnect_interval: Option<Duration>,
}

impl ClientConfig {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            reconnect_interval: Some(DEFAULT_RECONNECT_INTERVAL),
        }
    }
}

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
    config: ClientConfig,
) -> (ClientHandle, impl Future<Output = Result<(), BoxError>>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = ClientHandle { sender };
    let future = tokio::spawn(async move {
        let (stream, _) = tokio_tungstenite::connect_async(&config.url).await?;
        tracing::info!("connected to {}", config.url);
        let actor = ClientActor {
            client,
            receiver,
            stream,
            heartbeat: Instant::now(),
            config,
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
    config: ClientConfig,
    heartbeat: Instant,
}

impl<C: Client> ClientActor<C> {
    async fn run(mut self) -> Result<(), BoxError> {
        use futures::StreamExt;
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    let should_continue = self.handle_message(message).await?;
                    if !should_continue {
                        return Ok(())
                    }
                }
                Some(message) = self.stream.next() => {
                    let result = match message {
                        Ok(message) => self.handle_websocket_message(message).await,
                        Err(err) => Err(err.into()),
                    };
                    match result {
                        Ok(true) => continue,
                        Ok(false) => {
                            // TODO: Should I reconnect here?
                            return Ok(())
                        },
                        Err(err) => {
                            tracing::error!("connection error: {}", err);
                            if let Some(reconnect_interval) = self.config.reconnect_interval {
                                self.reconnect(reconnect_interval).await;
                            }
                        }
                    };
                }
                _ = ping_interval.tick() => {
                    self.stream.send(tungstenite::Message::Ping(Vec::new())).await?;
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn reconnect(&mut self, reconnect_interval: Duration) {
        tracing::info!("reconnecting...");
        loop {
            let result = tokio_tungstenite::connect_async(&self.config.url).await;
            match result {
                Ok((stream, _)) => {
                    tracing::error!("successfully reconnected");
                    self.stream = stream;
                    self.heartbeat = Instant::now();
                    return;
                }
                Err(err) => {
                    tracing::error!("reconnecting failed due to {}. will retry in {}s", err, reconnect_interval.as_secs());
                    tokio::time::sleep(reconnect_interval).await;
                }
            };
        }
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
