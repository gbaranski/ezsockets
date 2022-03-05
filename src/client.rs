use crate::BoxError;
use crate::CloseFrame;
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
    url: Url,
    reconnect_interval: Option<Duration>,
    headers: http::HeaderMap<http::HeaderValue>,
}

impl ClientConfig {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            reconnect_interval: Some(DEFAULT_RECONNECT_INTERVAL),
            headers: http::HeaderMap::new(),
        }
    }

    pub fn basic(mut self, username: &str, password: &str) -> Self {
        let credentials = base64::encode(format!("{username}:{password}"));
        self.headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&credentials).unwrap(),
        );
        self
    }

    fn connect_http_request(&self) -> http::Request<()> {
        let mut http_request = http::Request::builder()
            .uri(self.url.as_str())
            .body(())
            .unwrap();
        while let Some((key, value)) = self.headers.iter().next() {
            http_request.headers_mut().insert(key, value.to_owned());
        }
        http_request
    }
}

type WebSocketStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[async_trait]
pub trait Client: Send {
    async fn text(&mut self, text: String) -> Result<Option<Message>, BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<Message>, BoxError>;
    async fn closed(&mut self) -> Result<(), BoxError>;
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
        let http_request = config.connect_http_request();
        let (stream, _) = tokio_tungstenite::connect_async(http_request).await?;
        tracing::info!("connected to {}", config.url);
        let mut actor = ClientActor {
            client,
            receiver,
            stream,
            heartbeat: Instant::now(),
            config,
        };
        loop {
            let result = actor.run().await;
            match result {
                Ok(Some(CloseFrame { code, reason })) => {
                    tracing::info!(?code, %reason, "connection closed");
                }
                Ok(None) => {
                    tracing::info!("connection closed");
                    break; // TODO: Should I really break here?
                }
                Err(err) => tracing::warn!("connection error: {err}"),
            };
            actor.reconnect().await;
        }
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
    async fn run(&mut self) -> Result<Option<CloseFrame>, BoxError> {
        use futures::StreamExt;
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    let message = match message {
                        Message::Text(text) => tungstenite::Message::Text(text),
                        Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
                        Message::Close(frame) => {
                            self.stream.close(frame.clone().map(CloseFrame::into)).await?;
                            return Ok(frame);
                        }
                    };
                    self.stream.send(message).await?;
                }
                Some(message) = self.stream.next() => {
                    let message = message?;
                    tracing::debug!("received message: {:?}", message);
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
                            self.client.closed().await?;
                            return Ok(frame.map(CloseFrame::from));
                        }
                        tungstenite::Message::Frame(_) => todo!(),
                    };
                    if let Some(message) = message {
                        self.stream.send(message.into()).await?;
                    }
                }
                _ = ping_interval.tick() => {
                    self.stream.send(tungstenite::Message::Ping(Vec::new())).await?;
                }
                else => break,
            }
        }

        Ok(None)
    }

    async fn reconnect(&mut self) {
        let reconnect_interval = self
            .config
            .reconnect_interval
            .expect("reconnect interval should be set for reconnecting");
        for i in 1.. {
            tracing::info!("reconnecting attempt no: {}...", i);
            let result = tokio_tungstenite::connect_async(&self.config.url).await;
            match result {
                Ok((stream, _)) => {
                    tracing::error!("successfully reconnected");
                    self.stream = stream;
                    self.heartbeat = Instant::now();
                    return;
                }
                Err(err) => {
                    tracing::error!(
                        "reconnecting failed due to {}. will retry in {}s",
                        err,
                        reconnect_interval.as_secs()
                    );
                    tokio::time::sleep(reconnect_interval).await;
                }
            };
        }
    }
}
