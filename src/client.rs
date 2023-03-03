use crate::socket::Config;
use crate::Error;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
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
            http::HeaderValue::from_str(&format!("Basic {credentials}")).unwrap(),
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
pub trait ClientExt: Send {
    type Params: std::fmt::Debug + Send;

    async fn text(&mut self, text: String) -> Result<(), Error>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    async fn call(&mut self, params: Self::Params) -> Result<(), Error>;

    /// Called when the connection is closed.
    /// 
    /// For reconnections, use `ClientConfig::reconnect_interval`.
    async fn close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct Client<E: ClientExt> {
    socket: mpsc::UnboundedSender<Message>,
    calls: mpsc::UnboundedSender<E::Params>,
}

impl<E: ClientExt> Clone for Client<E> {
    fn clone(&self) -> Self {
        Self {
            socket: self.socket.clone(),
            calls: self.calls.clone(),
        }
    }
}

impl<E: ClientExt> From<Client<E>> for mpsc::UnboundedSender<E::Params> {
    fn from(client: Client<E>) -> Self {
        client.calls
    }
}

impl<E: ClientExt> Client<E> {
    pub fn text(&self, text: String) {
        self.socket.send(Message::Text(text)).unwrap();
    }

    pub fn binary(&self, bytes: Vec<u8>) {
        self.socket.send(Message::Binary(bytes)).unwrap();
    }

    pub fn call(&self, message: E::Params) {
        self.calls.send(message).unwrap();
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the Params which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> E::Params,
    ) -> R {
        let (sender, receiver) = oneshot::channel();
        let params = f(sender);

        self.calls.send(params).unwrap();
        receiver.await.unwrap()
    }
}

pub async fn connect<E: ClientExt + 'static>(
    client_fn: impl FnOnce(Client<E>) -> E,
    config: ClientConfig,
) -> (Client<E>, impl Future<Output = Result<(), Error>>) {
    let (socket_sender, socket_receiver) = mpsc::unbounded_channel();
    let (call_sender, call_receiver) = mpsc::unbounded_channel();
    let handle = Client {
        socket: socket_sender,
        calls: call_sender,
    };
    let client = client_fn(handle.clone());
    let future = tokio::spawn(async move {
        let http_request = config.connect_http_request();
        tracing::info!("connecting to {}...", config.url);
        let (stream, _) = tokio_tungstenite::connect_async(http_request).await?;
        let socket = Socket::new(stream, Config::default());
        tracing::info!("connected to {}", config.url);
        let mut actor = ClientActor {
            client,
            socket_receiver,
            call_receiver,
            socket,
            heartbeat: Instant::now(),
            config,
        };
        actor.run().await?;
        Ok(())
    });
    let future = async move { future.await.unwrap() };
    (handle, future)
}

struct ClientActor<E: ClientExt> {
    client: E,
    socket_receiver: mpsc::UnboundedReceiver<Message>,
    call_receiver: mpsc::UnboundedReceiver<E::Params>,
    socket: Socket,
    config: ClientConfig,
    heartbeat: Instant,
}

impl<E: ClientExt> ClientActor<E> {
    async fn run(&mut self) -> Result<(), Error> {
        loop {
            tokio::select! {
                Some(message) = self.socket_receiver.recv() => {
                    self.socket.send(message.clone()).await;
                    if let Message::Close(_frame) = message {
                        return Ok(())
                    }
                }
                Some(params) = self.call_receiver.recv() => {
                    self.client.call(params).await?;
                }
                result = self.socket.stream.recv() => {
                    match result {
                        Some(Ok(message)) => {
                             match message.to_owned() {
                                Message::Text(text) => self.client.text(text).await?,
                                Message::Binary(bytes) => self.client.binary(bytes).await?,
                                Message::Close(_frame) => {
                                    self.client.close().await?;
                                    self.reconnect().await;
                                }
                            };
                        }
                        Some(Err(error)) => {
                            tracing::error!("connection error: {error}");
                        }
                        None => {
                            self.client.close().await?;
                            self.reconnect().await;
                        }
                    };
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn reconnect(&mut self) {
        let reconnect_interval = self
            .config
            .reconnect_interval
            .expect("reconnect interval should be set for reconnecting");
        tracing::info!("reconnecting in {}s", reconnect_interval.as_secs());
        for i in 1.. {
            tokio::time::sleep(reconnect_interval).await;
            tracing::info!("reconnecting attempt no: {}...", i);
            let connect_http_request = self.config.connect_http_request();
            let result = tokio_tungstenite::connect_async(connect_http_request).await;
            match result {
                Ok((socket, _)) => {
                    tracing::info!("successfully reconnected");
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
                }
            };
        }
    }
}
