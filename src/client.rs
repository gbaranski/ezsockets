use crate::BoxError;
use crate::CloseFrame;
use crate::Message;
use crate::Socket;
use crate::socket::Config;
use async_trait::async_trait;
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
            .method("GET")
            .header("Host", self.url.host().unwrap().to_string())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .unwrap();
        for (key, value) in self.headers.clone() {
            http_request.headers_mut().insert(key.unwrap(), value);
        }
        http_request
    }
}

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
        tracing::info!("connecting to {}...", config.url);
        let (stream, _) = tokio_tungstenite::connect_async(http_request).await?;
        let socket = Socket::new(stream, Config::default());
        tracing::info!("connected to {}", config.url);
        let mut actor = ClientActor {
            client,
            receiver,
            socket,
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
    socket: Socket,
    config: ClientConfig,
    heartbeat: Instant,
}

impl<C: Client> ClientActor<C> {
    async fn run(&mut self) -> Result<Option<CloseFrame>, BoxError> {
        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    self.socket.send(message.clone().into()).await;
                    if let Message::Close(frame) = message {
                       return Ok(frame)
                    }
                }
                Some(message) = self.socket.recv() => {
                    let response = match message.to_owned() {
                        Message::Text(text) => self.client.text(text).await?,
                        Message::Binary(bytes) => self.client.binary(bytes).await?,
                        Message::Close(_frame) => {
                            // TODO: pass close frame to closed()
                            self.client.closed().await?;
                            None
                        }
                    };
                    if let Some(message) = response.to_owned() {
                        self.socket.send(message.into()).await;
                    }
                    if let Message::Close(frame) = message {
                        return Ok(frame);
                    }
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
                Ok((socket, _)) => {
                    tracing::error!("successfully reconnected");
                    let socket = Socket::new(socket, Config::default());
                    self.socket = socket;
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
