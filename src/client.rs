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

use crate::socket::{InMessage, MessageSignal, SocketConfig};
use crate::CloseFrame;
use crate::Error;
use crate::Message;
use crate::RawMessage;
use crate::Request;
use crate::Socket;
use async_trait::async_trait;
use base64::Engine;
use enfync::Handle;
use futures::{FutureExt, SinkExt, StreamExt};
use http::header::HeaderName;
use http::HeaderValue;
use std::fmt;
use std::time::Duration;
use tokio_tungstenite_wasm::Error as WSError;
use url::Url;

#[cfg(not(target_family = "wasm"))]
use tokio::time::sleep;

#[cfg(target_family = "wasm")]
use wasmtimer::tokio::sleep;

pub const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::new(5, 0);

#[derive(Debug)]
pub struct ClientConfig {
    url: Url,
    max_initial_connect_attempts: usize,
    max_reconnect_attempts: usize,
    reconnect_interval: Duration,
    headers: http::HeaderMap,
    socket_config: Option<SocketConfig>,
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
            max_initial_connect_attempts: usize::MAX,
            max_reconnect_attempts: usize::MAX,
            reconnect_interval: DEFAULT_RECONNECT_INTERVAL,
            headers: http::HeaderMap::new(),
            socket_config: None,
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

    /// Set the maximum number of connection attempts when starting a client.
    ///
    /// Defaults to infinite.
    pub fn max_initial_connect_attempts(mut self, max_initial_connect_attempts: usize) -> Self {
        self.max_initial_connect_attempts = max_initial_connect_attempts;
        self
    }

    /// Set the maximum number of attempts when reconnecting.
    ///
    /// Defaults to infinite.
    pub fn max_reconnect_attempts(mut self, max_reconnect_attempts: usize) -> Self {
        self.max_reconnect_attempts = max_reconnect_attempts;
        self
    }

    /// Set the reconnect interval.
    pub fn reconnect_interval(mut self, reconnect_interval: Duration) -> Self {
        self.reconnect_interval = reconnect_interval;
        self
    }

    /// Set the socket's configuration.
    pub fn socket_config(mut self, socket_config: SocketConfig) -> Self {
        self.socket_config = Some(socket_config);
        self
    }

    /// Get the config's headers.
    pub fn headers(&self) -> &http::HeaderMap {
        &self.headers
    }

    /// Extract a Websockets HTTP request.
    pub fn connect_http_request(&self) -> Request {
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

    /// Extract the URL request.
    ///
    /// This is needed for WASM clients, where building HTTP requests is deferred to the `web_sys::Websocket` implementation.
    pub fn connect_url(&self) -> &str {
        self.url.as_str()
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

    /// Handler for text messages from the server.
    ///
    /// Returning an error will force-close the client.
    async fn on_text(&mut self, text: String) -> Result<(), Error>;
    /// Handler for binary messages from the server.
    ///
    /// Returning an error will force-close the client.
    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program.
    ///
    /// Returning an error will force-close the client.
    ///
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error>;

    /// Called when the client successfully connected (or reconnected).
    ///
    /// Returning an error will force-close the client.
    async fn on_connect(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Called when the client fails a connection/reconnection attempt.
    ///
    /// Returning an error will force-close the client.
    ///
    /// By default, the client will continue trying to connect.
    /// Return [`ClientCloseMode::Close`] here to fully close instead.
    async fn on_connect_fail(&mut self, _error: WSError) -> Result<ClientCloseMode, Error> {
        Ok(ClientCloseMode::Reconnect)
    }

    /// Called when the connection is closed by the server.
    ///
    /// Returning an error will force-close the client.
    ///
    /// By default, the client will try to reconnect. Return [`ClientCloseMode::Close`] here to fully close instead.
    ///
    /// For reconnections, use `ClientConfig::reconnect_interval`.
    async fn on_close(&mut self, _frame: Option<CloseFrame>) -> Result<ClientCloseMode, Error> {
        Ok(ClientCloseMode::Reconnect)
    }

    /// Called when the connection is closed by the socket dying.
    ///
    /// Returning an error will force-close the client.
    ///
    /// By default, the client will try to reconnect. Return [`ClientCloseMode::Close`] here to fully close instead.
    ///
    /// For reconnections, use `ClientConfig::reconnect_interval`.
    async fn on_disconnect(&mut self) -> Result<ClientCloseMode, Error> {
        Ok(ClientCloseMode::Reconnect)
    }
}

/// Abstract interface used by clients to connect to servers.
///
/// The connector must expose a handle representing the runtime that the client will run on. The runtime should
/// be compatible with the connection method (e.g. `tokio` for `tokio_tungstenite::connect()`,
/// `wasm_bindgen_futures::spawn_local()` for a WASM connector, etc.).
#[async_trait]
pub trait ClientConnector {
    type Handle: enfync::Handle;
    type Message: Into<RawMessage> + From<RawMessage> + std::fmt::Debug + Send + 'static;
    type WSError: std::error::Error + Into<WSError> + Send;
    type Socket: SinkExt<Self::Message, Error = Self::WSError>
        + StreamExt<Item = Result<Self::Message, Self::WSError>>
        + Unpin
        + Send
        + 'static;

    /// Get the connector's runtime handle.
    fn handle(&self) -> Self::Handle;

    /// Connect to a websocket server.
    ///
    /// Returns `Err` if the request is invalid.
    async fn connect(&self, client_config: &ClientConfig) -> Result<Self::Socket, Self::WSError>;
}

/// An `ezsockets` client.
#[derive(Debug)]
pub struct Client<E: ClientExt> {
    to_socket_sender: async_channel::Sender<InMessage>,
    client_call_sender: async_channel::Sender<E::Call>,
}

impl<E: ClientExt> Clone for Client<E> {
    fn clone(&self) -> Self {
        Self {
            to_socket_sender: self.to_socket_sender.clone(),
            client_call_sender: self.client_call_sender.clone(),
        }
    }
}

impl<E: ClientExt> From<Client<E>> for async_channel::Sender<E::Call> {
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
    ) -> Result<MessageSignal, async_channel::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Text(text.into()));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send_blocking(inmessage)
            .map(|_| inmessage_signal)
    }

    /// Send a binary message to the server.
    ///
    /// Returns a `MessageSignal` which will report if sending succeeds/fails.
    pub fn binary(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<MessageSignal, async_channel::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Binary(bytes.into()));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send_blocking(inmessage)
            .map(|_| inmessage_signal)
    }

    /// Call a custom method on the Client.
    ///
    /// Refer to `ClientExt::on_call`.
    pub fn call(&self, message: E::Call) -> Result<(), async_channel::SendError<E::Call>> {
        self.client_call_sender.send_blocking(message)
    }

    /// Call a custom method on the Client, with a reply from the `ClientExt::on_call`.
    ///
    /// This works just as syntactic sugar for `Client::call(sender)`
    pub async fn call_with<R: fmt::Debug>(
        &self,
        f: impl FnOnce(async_channel::Sender<R>) -> E::Call,
    ) -> Option<R> {
        let (sender, receiver) = async_channel::bounded(1usize);
        let call = f(sender);

        let Ok(_) = self.client_call_sender.send(call).await else {
            return None;
        };
        let Ok(result) = receiver.recv().await else {
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
    ) -> Result<MessageSignal, async_channel::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Close(frame));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send_blocking(inmessage)
            .map(|_| inmessage_signal)
    }
}

/// Connect to a websocket server using the default client connector.
/// - Requires feature `native_client`.
/// - May only be invoked from within a tokio runtime.
#[cfg(feature = "native_client")]
#[cfg_attr(docsrs, doc(cfg(feature = "native_client")))]
pub async fn connect<E: ClientExt + 'static>(
    client_fn: impl FnOnce(Client<E>) -> E,
    config: ClientConfig,
) -> (
    Client<E>,
    impl std::future::Future<Output = Result<(), Error>>,
) {
    let client_connector = crate::ClientConnectorTokio::default();
    let (handle, mut future) = connect_with(client_fn, config, client_connector);
    let future = async move {
        future
            .extract()
            .await
            .unwrap_or(Err("client actor crashed".into()))
    };
    (handle, future)
}

/// Connect to a websocket server with the provided client connector.
pub fn connect_with<E: ClientExt + 'static>(
    client_fn: impl FnOnce(Client<E>) -> E,
    config: ClientConfig,
    client_connector: impl ClientConnector + Send + Sync + 'static,
) -> (Client<E>, enfync::PendingResult<Result<(), Error>>) {
    let (to_socket_sender, mut to_socket_receiver) = async_channel::unbounded();
    let (client_call_sender, client_call_receiver) = async_channel::unbounded();
    let handle = Client {
        to_socket_sender,
        client_call_sender,
    };
    let mut client = client_fn(handle.clone());
    let runtime_handle = client_connector.handle();
    let future = runtime_handle.spawn(async move {
        tracing::info!("connecting to {}...", config.url);
        let Some(socket) = client_connect(
            config.max_initial_connect_attempts,
            &config,
            &client_connector,
            &mut to_socket_receiver,
            &mut client,
        )
        .await?
        else {
            return Ok(());
        };
        tracing::info!("connected to {}", config.url);

        let mut actor = ClientActor {
            client,
            to_socket_receiver,
            client_call_receiver,
            config,
            client_connector,
        };
        actor.run(Some(socket)).await?;
        Ok(())
    });
    (handle, future)
}

struct ClientActor<E: ClientExt, C: ClientConnector> {
    client: E,
    to_socket_receiver: async_channel::Receiver<InMessage>,
    client_call_receiver: async_channel::Receiver<E::Call>,
    config: ClientConfig,
    client_connector: C,
}

impl<E: ClientExt, C: ClientConnector> ClientActor<E, C> {
    async fn run(&mut self, mut socket_shuttle: Option<Socket>) -> Result<(), Error> {
        loop {
            let Some(mut socket) = socket_shuttle.take() else {
                return Ok(());
            };
            futures::select! {
                res = self.to_socket_receiver.recv().fuse() => {
                    let Ok(inmessage) = res else {
                        break;
                    };
                    socket_shuttle = Self::handle_outgoing_msg(socket, inmessage).await?;
                }
                res = self.client_call_receiver.recv().fuse() => {
                    let Ok(call) = res else {
                        break;
                    };
                    self.client.on_call(call).await?;
                    socket_shuttle = Some(socket);
                }
                result = socket.stream.recv().fuse() => {
                    socket_shuttle = self.handle_incoming_msg(socket, result).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_outgoing_msg(
        mut socket: Socket,
        inmessage: InMessage,
    ) -> Result<Option<Socket>, Error> {
        let closed_self = matches!(inmessage.message, Some(Message::Close(_)));
        if socket.send(inmessage).await.is_err() {
            let result = socket.await_sink_close().await;
            if let Err(err) = &result {
                tracing::warn!(
                    ?err,
                    "encountered sink closing error when trying to send a message"
                );
            }
            match result {
                Err(WSError::ConnectionClosed)
                | Err(WSError::AlreadyClosed)
                | Err(WSError::Io(_))
                | Err(WSError::Tls(_)) => {
                    // either:
                    // A) The connection was closed via the close protocol, so we will allow the stream to
                    //    handle it.
                    // B) We already tried and failed to submit another message, so now we are
                    //    waiting for other parts of the select! to shut us down.
                    // C) An IO error means the connection closed unexpectedly, so we can try to reconnect when
                    //    the stream fails.
                }
                Err(_) if !closed_self => {
                    return Err(Error::from("unexpected sink error, aborting client actor"))
                }
                _ => (),
            }
        }
        if closed_self {
            tracing::debug!("client closed itself");
            return Ok(None);
        }

        Ok(Some(socket))
    }

    async fn handle_incoming_msg(
        &mut self,
        socket: Socket,
        result: Option<Result<Message, WSError>>,
    ) -> Result<Option<Socket>, Error> {
        match result {
            Some(Ok(message)) => {
                match message.to_owned() {
                    Message::Text(text) => self.client.on_text(text).await?,
                    Message::Binary(bytes) => self.client.on_binary(bytes).await?,
                    Message::Close(frame) => {
                        tracing::debug!("client closed by server");
                        match self.client.on_close(frame).await? {
                            ClientCloseMode::Reconnect => {
                                std::mem::drop(socket);
                                let Some(socket) = client_connect(
                                    self.config.max_reconnect_attempts,
                                    &self.config,
                                    &self.client_connector,
                                    &mut self.to_socket_receiver,
                                    &mut self.client,
                                )
                                .await?
                                else {
                                    return Ok(None);
                                };
                                return Ok(Some(socket));
                            }
                            ClientCloseMode::Close => return Ok(None),
                        }
                    }
                };
            }
            Some(Err(error)) => {
                let error = Error::from(error);
                tracing::warn!("connection error: {error}");
            }
            None => {
                tracing::debug!("client socket died");
                match self.client.on_disconnect().await? {
                    ClientCloseMode::Reconnect => {
                        std::mem::drop(socket);
                        let Some(socket) = client_connect(
                            self.config.max_reconnect_attempts,
                            &self.config,
                            &self.client_connector,
                            &mut self.to_socket_receiver,
                            &mut self.client,
                        )
                        .await?
                        else {
                            return Ok(None);
                        };
                        return Ok(Some(socket));
                    }
                    ClientCloseMode::Close => return Ok(None),
                }
            }
        }

        Ok(Some(socket))
    }
}

/// Returns Ok(Some(socket)) if connecting succeeded, Ok(None) if the client closed itself, and `Err` if an error occurred.
async fn client_connect<E: ClientExt, Connector: ClientConnector>(
    max_attempts: usize,
    config: &ClientConfig,
    client_connector: &Connector,
    to_socket_receiver: &mut async_channel::Receiver<InMessage>,
    client: &mut E,
) -> Result<Option<Socket>, Error> {
    for i in 1.. {
        // handle incoming user messages
        // - It is important to do this at least once after a disconnect so users can guarantee that messages are not
        //   sent 'across' a reconnect cycle. They can achieve that, in combination with this step, by manually
        //   preventing messages from being sent to to_socket_receiver between on_disconnect/on_close and on_connect.
        loop {
            let in_message = to_socket_receiver.try_recv();
            match in_message {
                Ok(inmessage) => match &inmessage.message {
                    Some(Message::Close(frame)) => {
                        tracing::debug!(?frame, "client closed itself while connecting");
                        return Ok(None);
                    }
                    _ => {
                        tracing::warn!("client is connecting, discarding message from user");
                        continue;
                    }
                },
                Err(async_channel::TryRecvError::Empty) => break,
                Err(async_channel::TryRecvError::Closed) => {
                    tracing::warn!("client is dead, aborting connection attempts");
                    return Err(Error::from("client died while trying to connect"));
                }
            }
        }

        // connection attempt
        tracing::info!("connecting attempt no: {}...", i);
        let result = client_connector.connect(config).await;
        match result {
            Ok(socket_impl) => {
                tracing::info!("successfully connected");
                client.on_connect().await?;
                let socket = Socket::new(
                    socket_impl,
                    config.socket_config.clone().unwrap_or_default(),
                    client_connector.handle(),
                );
                return Ok(Some(socket));
            }
            Err(err) => {
                tracing::warn!("connecting failed due to {}", err);
                match client.on_connect_fail(err.into()).await? {
                    ClientCloseMode::Reconnect => {
                        tracing::debug!("will retry in {}s", config.reconnect_interval.as_secs());
                    }
                    ClientCloseMode::Close => {
                        tracing::debug!("client closed itself after a connection failure");
                        return Ok(None);
                    }
                }
            }
        };

        // abort if we have reached the max attempts
        if i >= max_attempts {
            return Err(Error::from(format!(
                "failed to connect after {} attempt(s), aborting...",
                i
            )));
        }

        // wait for the connect interval
        sleep(config.reconnect_interval).await;
    }

    Err(Error::from("client failed to connect"))
}
