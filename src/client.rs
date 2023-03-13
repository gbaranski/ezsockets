//! ## Get started
//!
//! The code below represents a simple client that redirects stdin to the WebSocket server.
//! ```rust
//! use async_trait::async_trait;
//! use ezsockets::ClientConfig;
//! use std::io::BufRead;
//! use url::Url;
//!
//! struct Client {}
//!
//! #[async_trait]
//! impl ezsockets::ClientExt for Client {
//!     type Params = ();
//!
//!     async fn text(&mut self, text: String) -> Result<(), ezsockets::Error> {
//!         tracing::info!("received message: {text}");
//!         Ok(())
//!     }
//!
//!     async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
//!         tracing::info!("received bytes: {bytes:?}");
//!         Ok(())
//!     }
//!
//!     async fn call(&mut self, params: Self::Params) -> Result<(), ezsockets::Error> {
//!         let () = params;
//!         Ok(())
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     tracing_subscriber::fmt::init();
//!     let config = ClientConfig::new("ws://localhost:8080/websocket");
//!     let (handle, future) = ezsockets::connect(|_client| Client { }, config).await;
//!     tokio::spawn(async move {
//!         future.await.unwrap();
//!     });
//!     let stdin = std::io::stdin();
//!     let lines = stdin.lock().lines();
//!     for line in lines {
//!         let line = line.unwrap();
//!         tracing::info!("sending {line}");
//!         handle.text(line);
//!     }
//! }
//!
//! ```

use crate::socket::Config;
use crate::CloseFrame;
use crate::Error;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use base64::Engine;
use http::header::HeaderName;
use http::HeaderValue;
use std::fmt;
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use url::Url;

pub const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::new(5, 0);

#[derive(Debug)]
pub struct ClientConfig {
    url: Url,
    reconnect_interval: Option<Duration>,
    headers: http::HeaderMap,
}

impl ClientConfig {
    /// If invalid URL is passed, this function will panic.
    /// In order to handle invalid URL, parse URL on your side, and pass `url::Url` directly.
    pub fn new<U>(url: U) -> Self
    where
        U: TryInto<Url>,
        U::Error: fmt::Debug,
    {
        let url = url.try_into().expect("invalid URL");
        Self {
            url,
            reconnect_interval: Some(DEFAULT_RECONNECT_INTERVAL),
            headers: http::HeaderMap::new(),
        }
    }

    pub fn basic(mut self, username: impl fmt::Display, password: impl fmt::Display) -> Self {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        self.headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Basic {credentials}")).unwrap(),
        );
        self
    }

    /// If invalid(outside of visible ASCII characters ranged between 32-127) token is passed, this function will panic.
    pub fn bearer(mut self, token: impl fmt::Display) -> Self {
        self.headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("token contains invalid character"),
        );
        self
    }

    /// If you suppose the header name or value might be invalid, create `http::header::HeaderName` and `http::header::HeaderValue` on your side, and then pass it to this function
    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        // Those errors are handled by the `expect` calls.
        // Possibly a better way to do this?
        let name = <HeaderName as TryFrom<K>>::try_from(key)
            .map_err(Into::into)
            .expect("invalid header name");
        let value = <HeaderValue as TryFrom<V>>::try_from(value)
            .map_err(Into::into)
            .expect("invalid header value");
        self.headers.insert(name, value);
        self
    }

    pub fn reconnect_interval(mut self, reconnect_interval: Duration) -> Self {
        self.reconnect_interval = Some(reconnect_interval);
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
    /// Type the custom call - parameters passed to `on_call`.
    type Call: fmt::Debug + Send;

    /// Handler for text messages from the server.
    async fn on_text(&mut self, text: String) -> Result<(), Error>;
    /// Handler for binary messages from the server.
    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program.
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, params: Self::Call) -> Result<(), Error>;

    /// Called when the client successfully connected(or reconnected).
    async fn on_connect(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Called when the connection is closed.
    ///
    /// For reconnections, use `ClientConfig::reconnect_interval`(enabled by default).
    async fn on_close(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct Client<E: ClientExt> {
    socket: mpsc::UnboundedSender<Message>,
    calls: mpsc::UnboundedSender<E::Call>,
}

impl<E: ClientExt> Clone for Client<E> {
    fn clone(&self) -> Self {
        Self {
            socket: self.socket.clone(),
            calls: self.calls.clone(),
        }
    }
}

impl<E: ClientExt> From<Client<E>> for mpsc::UnboundedSender<E::Call> {
    fn from(client: Client<E>) -> Self {
        client.calls
    }
}

impl<E: ClientExt> Client<E> {
    /// Send a text message to the server.
    pub fn text(&self, text: String) {
        self.socket.send(Message::Text(text)).unwrap();
    }

    /// Send a binary message to the server.
    pub fn binary(&self, bytes: Vec<u8>) {
        self.socket.send(Message::Binary(bytes)).unwrap();
    }

    /// Call a custom method on the Client.
    /// Refer to `ClientExt::on_call`.
    pub fn call(&self, message: E::Call) {
        self.calls.send(message).unwrap();
    }

    /// Call a custom method on the Client, with a reply from the `ClientExt::on_call`.
    /// This works just as a syntactic sugar for `Client::call(sender)`
    pub async fn call_with<R: fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> E::Call,
    ) -> R {
        let (sender, receiver) = oneshot::channel();
        let params = f(sender);

        self.calls.send(params).unwrap();
        receiver.await.unwrap()
    }

    /// Disconnect client from the server.
    /// Optionally pass a frame with reason and code.
    pub async fn close(self, frame: Option<CloseFrame>) {
        self.socket.send(Message::Close(frame)).unwrap();
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
    let mut client = client_fn(handle.clone());
    let future = tokio::spawn(async move {
        let http_request = config.connect_http_request();
        tracing::info!("connecting to {}...", config.url);
        let (stream, _) = tokio_tungstenite::connect_async(http_request).await?;
        if let Err(err) = client.on_connect().await {
            tracing::error!("calling `connected()` failed due to {}", err);
        }
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
    call_receiver: mpsc::UnboundedReceiver<E::Call>,
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
                    self.client.on_call(params).await?;
                }
                result = self.socket.stream.recv() => {
                    match result {
                        Some(Ok(message)) => {
                             match message.to_owned() {
                                Message::Text(text) => self.client.on_text(text).await?,
                                Message::Binary(bytes) => self.client.on_binary(bytes).await?,
                                Message::Close(_frame) => {
                                    self.client.on_close().await?;
                                    self.reconnect().await;
                                }
                            };
                        }
                        Some(Err(error)) => {
                            tracing::error!("connection error: {error}");
                        }
                        None => {
                            self.client.on_close().await?;
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
                    if let Err(err) = self.client.on_connect().await {
                        tracing::error!("calling `connected()` failed due to {}", err);
                    }
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
