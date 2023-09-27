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
//!     type Call = ();
//!
//!     async fn on_text(&mut self, text: String) -> Result<(), ezsockets::Error> {
//!         tracing::info!("received message: {text}");
//!         Ok(())
//!     }
//!
//!     async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
//!         tracing::info!("received bytes: {bytes:?}");
//!         Ok(())
//!     }
//!
//!     async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> {
//!         let () = call;
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

use crate::socket::{Config, InMessage, MessageSignal};
use crate::CloseFrame;
use crate::Error;
use crate::Message;
use crate::Request;
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

    /// Add 'basic' header.
    /// Note that additional headers are not supported by the websockets spec, so may not be supported by all
    /// implementations.
    pub fn basic(mut self, username: impl fmt::Display, password: impl fmt::Display) -> Self {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        self.headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Basic {credentials}")).unwrap(),
        );
        self
    }

    /// Add 'bearer' header.
    /// If invalid(outside of visible ASCII characters ranged between 32-127) token is passed, this function will panic.
    /// Note that additional headers are not supported by the websockets spec, so may not be supported by all
    /// implementations.
    pub fn bearer(mut self, token: impl fmt::Display) -> Self {
        self.headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("token contains invalid character"),
        );
        self
    }

    /// Add custom header.
    /// If you suppose the header name or value might be invalid, create `http::header::HeaderName` and
    /// `http::header::HeaderValue` on your side, and then pass it to this function.
    /// Note that additional headers are not supported by the websockets spec, so may not be supported by all
    /// implementations.
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

    /// Insert query parameters into the connection request URI.
    /// Query parameters are supported by the websockets spec, so they will always be available to the connecting server.
    /// Decode query parameters in `ServerExt::on_connect()` with
    /// `form_urlencoded::parse(request.uri().query().unwrap().as_bytes())` using the `form_urlencoded` crate.
    pub fn query_parameter(mut self, key: &str, value: &str) -> Self {
        self.url.query_pairs_mut().append_pair(key, value);
        self
    }

    pub fn reconnect_interval(mut self, reconnect_interval: Duration) -> Self {
        self.reconnect_interval = Some(reconnect_interval);
        self
    }

    fn connect_http_request(&self) -> Request {
        let mut http_request = Request::builder()
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

#[derive(Debug, Clone)]
pub enum ClientCloseMode {
    Reconnect,
    Close,
}

#[async_trait]
pub trait ClientExt: Send {
    /// Type the custom call - parameters passed to `on_call`.
    type Call: Send;

    /// Handler for text messages from the server. Returning an error will force-close the client.
    async fn on_text(&mut self, text: String) -> Result<(), Error>;
    /// Handler for binary messages from the server. Returning an error will force-close the client.
    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program. Returning an error will force-close the client.
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error>;

    /// Called when the client successfully connected(or reconnected). Returned errors will be ignored.
    async fn on_connect(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Called when the connection is closed by the server. Returning an error will force-close the client.
    ///
    /// By default, the client will try to reconnect. Return [`ClientCloseMode::Close`] here to fully close instead.
    ///
    /// For reconnections, use `ClientConfig::reconnect_interval`(enabled by default).
    async fn on_close(&mut self, _frame: Option<CloseFrame>) -> Result<ClientCloseMode, Error> {
        Ok(ClientCloseMode::Reconnect)
    }

    /// Called when the connection is closed by the socket dying.
    ///
    /// By default, the client will try to reconnect. Return [`ClientCloseMode::Close`] here to fully close instead.
    ///
    /// For reconnections, use `ClientConfig::reconnect_interval`(enabled by default).
    async fn on_disconnect(&mut self) -> Result<ClientCloseMode, Error> {
        Ok(ClientCloseMode::Reconnect)
    }
}

#[derive(Debug)]
pub struct Client<E: ClientExt> {
    to_socket_sender: mpsc::UnboundedSender<InMessage>,
    client_call_sender: mpsc::UnboundedSender<E::Call>,
}

impl<E: ClientExt> Clone for Client<E> {
    fn clone(&self) -> Self {
        Self {
            to_socket_sender: self.to_socket_sender.clone(),
            client_call_sender: self.client_call_sender.clone(),
        }
    }
}

impl<E: ClientExt> From<Client<E>> for mpsc::UnboundedSender<E::Call> {
    fn from(client: Client<E>) -> Self {
        client.client_call_sender
    }
}

impl<E: ClientExt> Client<E> {
    /// Send a text message to the server.
    ///
    /// Returns a `MessageSignal` which will report if sending succeeds/fails.
    pub fn text(
        &self,
        text: impl Into<String>,
    ) -> Result<MessageSignal, mpsc::error::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Text(text.into()));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send(inmessage)
            .map(|_| inmessage_signal)
    }

    /// Send a binary message to the server.
    ///
    /// Returns a `MessageSignal` which will report if sending succeeds/fails.
    pub fn binary(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<MessageSignal, mpsc::error::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Binary(bytes.into()));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send(inmessage)
            .map(|_| inmessage_signal)
    }

    /// Call a custom method on the Client.
    ///
    /// Refer to `ClientExt::on_call`.
    pub fn call(&self, message: E::Call) -> Result<(), mpsc::error::SendError<E::Call>> {
        self.client_call_sender.send(message)
    }

    /// Call a custom method on the Client, with a reply from the `ClientExt::on_call`.
    ///
    /// This works just as syntactic sugar for `Client::call(sender)`
    pub async fn call_with<R: fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> E::Call,
    ) -> Option<R> {
        let (sender, receiver) = oneshot::channel();
        let call = f(sender);

        let Ok(_) = self.client_call_sender.send(call) else {
            return None;
        };
        let Ok(result) = receiver.await else {
            return None;
        };
        Some(result)
    }

    /// Disconnect client from the server. Returns an error if the client is already closed.
    ///
    /// Optionally pass a frame with reason and code.
    ///
    /// Returns a `MessageSignal` which will report if sending the close frame to the server succeeds/fails. If
    /// it fails, then the connection was already closed.
    pub fn close(
        &self,
        frame: Option<CloseFrame>,
    ) -> Result<MessageSignal, mpsc::error::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Close(frame));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send(inmessage)
            .map(|_| inmessage_signal)
    }
}

pub async fn connect<E: ClientExt + 'static>(
    client_fn: impl FnOnce(Client<E>) -> E,
    config: ClientConfig,
) -> (Client<E>, impl Future<Output = Result<(), Error>>) {
    let (to_socket_sender, to_socket_receiver) = mpsc::unbounded_channel();
    let (client_call_sender, client_call_receiver) = mpsc::unbounded_channel();
    let handle = Client {
        to_socket_sender,
        client_call_sender,
    };
    let mut client = client_fn(handle.clone());
    let future = tokio::spawn(async move {
        let http_request = config.connect_http_request();
        tracing::info!("connecting to {}...", config.url);
        let (stream, _) = tokio_tungstenite::connect_async(http_request).await?;
        if let Err(err) = client.on_connect().await {
            tracing::error!("calling on_connect() failed due to {}", err);
            return Err(err);
        }
        let socket = Socket::new(stream, Config::default());
        tracing::info!("connected to {}", config.url);
        let mut actor = ClientActor {
            client,
            to_socket_receiver,
            client_call_receiver,
            socket,
            heartbeat: Instant::now(),
            config,
        };
        actor.run().await?;
        Ok(())
    });
    let future = async move { future.await.unwrap_or(Err("client actor crashed".into())) };
    (handle, future)
}

struct ClientActor<E: ClientExt> {
    client: E,
    to_socket_receiver: mpsc::UnboundedReceiver<InMessage>,
    client_call_receiver: mpsc::UnboundedReceiver<E::Call>,
    socket: Socket,
    config: ClientConfig,
    heartbeat: Instant,
}

impl<E: ClientExt> ClientActor<E> {
    async fn run(&mut self) -> Result<(), Error> {
        loop {
            tokio::select! {
                Some(inmessage) = self.to_socket_receiver.recv() => {
                    let mut closed_self = matches!(inmessage.message, Some(Message::Close(_)));
                    if self.socket.send(inmessage).await.is_err() {
                        closed_self = true;
                    }
                    if closed_self {
                        tracing::trace!("client closed itself");
                        return Ok(())
                    }
                }
                Some(call) = self.client_call_receiver.recv() => {
                    self.client.on_call(call).await?;
                }
                result = self.socket.stream.recv() => {
                    match result {
                        Some(Ok(message)) => {
                             match message.to_owned() {
                                Message::Text(text) => self.client.on_text(text).await?,
                                Message::Binary(bytes) => self.client.on_binary(bytes).await?,
                                Message::Close(frame) => {
                                    tracing::trace!("client closed by server");
                                    match self.client.on_close(frame).await?
                                    {
                                        ClientCloseMode::Reconnect => { if !self.try_reconnect().await { return Ok(()) } }
                                        ClientCloseMode::Close => return Ok(())
                                    }
                                }
                            };
                        }
                        Some(Err(error)) => {
                            tracing::warn!("connection error: {error}");
                        }
                        None => {
                            tracing::trace!("client socket died");
                            match self.client.on_disconnect().await?
                            {
                                ClientCloseMode::Reconnect => { if !self.try_reconnect().await { return Ok(()) } }
                                ClientCloseMode::Close => return Ok(())
                            }
                        }
                    };
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn try_reconnect(&mut self) -> bool {
        let Some(reconnect_interval) = self.config.reconnect_interval else {
            tracing::warn!("no reconnect interval set, aborting reconnect attempt");
            return false;
        };
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
                        tracing::error!("calling on_connect() failed due to {}", err);
                    }
                    let socket = Socket::new(socket, Config::default());
                    self.socket = socket;
                    self.heartbeat = Instant::now();
                    return true;
                }
                Err(err) => {
                    tracing::warn!(
                        "reconnecting failed due to {}. will retry in {}s",
                        err,
                        reconnect_interval.as_secs()
                    );
                }
            };
        }

        false
    }
}
