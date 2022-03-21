use crate::socket::Config;
use crate::BoxError;
use crate::CloseFrame;
use crate::Message;
use crate::Socket;
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

#[derive(Debug)]
enum ClientMessage<M: std::fmt::Debug> {
    Socket(Message),
    Call(M),
}

#[async_trait]
pub trait ClientExt: Send {
    type Message: std::fmt::Debug + Send;

    async fn text(&mut self, text: String) -> Result<(), BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), BoxError>;
    async fn closed(&mut self) -> Result<(), BoxError>;
    async fn call(&mut self, message: Self::Message);
}

#[derive(Debug)]
pub struct Client<M: std::fmt::Debug> {
    sender: mpsc::UnboundedSender<ClientMessage<M>>,
}

impl<M: std::fmt::Debug> Clone for Client<M> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<M: std::fmt::Debug> Client<M> {
    pub async fn text(&self, text: String) {
        self.sender
            .send(ClientMessage::Socket(Message::Text(text)))
            .unwrap();
    }

    pub async fn binary(&self, text: String) {
        self.sender
            .send(ClientMessage::Socket(Message::Text(text)))
            .unwrap();
    }

    pub async fn call(&self, message: M) {
        self.sender.send(ClientMessage::Call(message)).unwrap();
    }
}

pub async fn connect<E: ClientExt + 'static>(
    client_fn: impl FnOnce(Client<E::Message>) -> E,
    config: ClientConfig,
) -> (Client<E::Message>, impl Future<Output = Result<(), BoxError>>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = Client { sender };
    let client = client_fn(handle.clone());
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

struct ClientActor<E: ClientExt> {
    client: E,
    receiver: mpsc::UnboundedReceiver<ClientMessage<E::Message>>,
    socket: Socket,
    config: ClientConfig,
    heartbeat: Instant,
}

impl<E: ClientExt> ClientActor<E> {
    async fn run(&mut self) -> Result<Option<CloseFrame>, BoxError> {
        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    match message {
                        ClientMessage::Socket(message) => {
                            self.socket.send(message.clone().into()).await;
                            if let Message::Close(frame) = message {
                                return Ok(frame)
                            }
                        }
                        ClientMessage::Call(message) => {
                            self.client.call(message).await;
                        }
                    }
                }
                Some(message) = self.socket.recv() => {
                    match message.to_owned() {
                        Message::Text(text) => self.client.text(text).await?,
                        Message::Binary(bytes) => self.client.binary(bytes).await?,
                        Message::Close(_frame) => self.client.closed().await?
                    };
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
