use crate::client::{ClientConfig, ClientConnector};
use crate::socket::{CloseFrame, Message, RawMessage};

impl<'t> From<tokio_tungstenite_wasm::CloseFrame<'t>> for CloseFrame {
    fn from(frame: tokio_tungstenite_wasm::CloseFrame) -> Self {
        Self {
            code: Into::<tungstenite::protocol::frame::coding::CloseCode>::into(Into::<u16>::into(frame.code)).into(),
            reason: frame.reason.into(),
        }
    }
}

impl<'t> From<CloseFrame> for tokio_tungstenite_wasm::CloseFrame<'t> {
    fn from(frame: CloseFrame) -> Self {
        Self {
            code: Into::<u16>::into(Into::<tungstenite::protocol::frame::coding::CloseCode>::into(frame.code)).into(),
            reason: frame.reason.into(),
        }
    }
}

impl From<RawMessage> for tokio_tungstenite_wasm::Message {
    fn from(message: RawMessage) -> Self {
        match message {
            RawMessage::Text(text) => Self::Text(text),
            RawMessage::Binary(bytes) => Self::Binary(bytes),
            RawMessage::Ping(_) => Self::Close(None),
            RawMessage::Pong(_) => Self::Close(None),
            RawMessage::Close(frame) => Self::Close(frame.map(CloseFrame::into)),
        }
    }
}

impl From<tokio_tungstenite_wasm::Message> for RawMessage {
    fn from(message: tokio_tungstenite_wasm::Message) -> Self {
        match message {
            tokio_tungstenite_wasm::Message::Text(text) => Self::Text(text),
            tokio_tungstenite_wasm::Message::Binary(bytes) => Self::Binary(bytes),
            tokio_tungstenite_wasm::Message::Close(frame) => Self::Close(frame.map(CloseFrame::from)),
        }
    }
}

impl From<Message> for tokio_tungstenite_wasm::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => tokio_tungstenite_wasm::Message::Text(text),
            Message::Binary(bytes) => tokio_tungstenite_wasm::Message::Binary(bytes),
            Message::Close(frame) => tokio_tungstenite_wasm::Message::Close(frame.map(CloseFrame::into)),
        }
    }
}

/// Implementation of [`ClientConnector`] for tokio runtimes.
#[derive(Clone)]
pub struct ClientConnectorWasm {
    handle: enfync::builtin::wasm::WASMHandle,
}

impl Default for ClientConnectorWasm {
    fn default() -> Self {
        let handle = enfync::builtin::wasm::WASMHandle::default();
        Self { handle }
    }
}

#[async_trait::async_trait]
impl ClientConnector for ClientConnectorWasm {
    type Handle = enfync::builtin::wasm::WASMHandle;
    type Message = tokio_tungstenite_wasm::Message;
    type WSError = tokio_tungstenite_wasm::Error;
    type Socket = tokio_tungstenite_wasm::WebSocketStream;
    type ConnectError = tokio_tungstenite_wasm::Error;

    /// Get the connector's runtime handle.
    fn handle(&self) -> Self::Handle {
        self.handle.clone()
    }

    /// Connect to a websocket server.
    ///
    /// Returns `Err` if the request is invalid.
    ///
    /// Panics if any headers were added to the client config. Websockets on browser does not support
    /// additional headers (use [`ClientConfig::query_parameter()`] instead).
    async fn connect(&self, config: &ClientConfig) -> Result<Self::Socket, Self::ConnectError> {
        if config.headers().len() > 0 {
            panic!("client may not submit HTTP headers in WASM connection requests");
        }
        let request_url = config.connect_url();
        let socket = tokio_tungstenite_wasm::connect(request_url).await?;
        Ok(socket)
    }
}
