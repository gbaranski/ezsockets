use crate::client::{ClientConfig, ClientConnector};
use crate::socket::{CloseFrame, Message, RawMessage};

use std::pin::Pin;
use std::task::{Context, Poll};
use tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;

impl<'t> From<tokio_tungstenite_wasm::CloseFrame<'t>> for CloseFrame {
    fn from(frame: tokio_tungstenite_wasm::CloseFrame) -> Self {
        Self {
            code: Into::<TungsteniteCloseCode>::into(Into::<u16>::into(frame.code)).into(),
            reason: frame.reason.into(),
        }
    }
}

impl<'t> From<CloseFrame> for tokio_tungstenite_wasm::CloseFrame<'t> {
    fn from(frame: CloseFrame) -> Self {
        Self {
            code: Into::<u16>::into(Into::<TungsteniteCloseCode>::into(frame.code)).into(),
            reason: frame.reason.into(),
        }
    }
}

impl From<RawMessage> for tokio_tungstenite_wasm::Message {
    fn from(message: RawMessage) -> Self {
        match message {
            RawMessage::Text(text) => Self::Text(text),
            RawMessage::Binary(bytes) => Self::Binary(bytes),
            RawMessage::Ping(_) => Self::Close(Some(tokio_tungstenite_wasm::CloseFrame {
                code: tokio_tungstenite_wasm::CloseCode::Abnormal,
                reason: "raw pings not supported".into(),
            })),
            RawMessage::Pong(_) => Self::Close(Some(tokio_tungstenite_wasm::CloseFrame {
                code: tokio_tungstenite_wasm::CloseCode::Abnormal,
                reason: "raw pongs not supported".into(),
            })),
            RawMessage::Close(frame) => Self::Close(frame.map(CloseFrame::into)),
        }
    }
}

impl From<tokio_tungstenite_wasm::Message> for RawMessage {
    fn from(message: tokio_tungstenite_wasm::Message) -> Self {
        match message {
            tokio_tungstenite_wasm::Message::Text(text) => Self::Text(text),
            tokio_tungstenite_wasm::Message::Binary(bytes) => Self::Binary(bytes),
            tokio_tungstenite_wasm::Message::Close(frame) => {
                Self::Close(frame.map(CloseFrame::from))
            }
        }
    }
}

impl From<Message> for tokio_tungstenite_wasm::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => tokio_tungstenite_wasm::Message::Text(text),
            Message::Binary(bytes) => tokio_tungstenite_wasm::Message::Binary(bytes),
            Message::Close(frame) => {
                tokio_tungstenite_wasm::Message::Close(frame.map(CloseFrame::into))
            }
        }
    }
}

/// Implementation of [`ClientConnector`] for WASM targets.
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
    type Socket = WebSocketStreamProxy;

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
    async fn connect(&self, config: &ClientConfig) -> Result<Self::Socket, Self::WSError> {
        if config.headers().len() > 0 {
            panic!("client may not submit HTTP headers in WASM connection requests");
        }
        let request_url = config.connect_url();
        let socket = wasm_client_connect(String::from(request_url)).await?;
        Ok(socket)
    }
}

/// Proxy websocket that allows access to a WASM websocket. We need this because
/// `tokio_tungstenite_wasm::WebSocketStream` is !Send.
pub struct WebSocketStreamProxy {
    inner: fragile::Fragile<tokio_tungstenite_wasm::WebSocketStream>,
}

impl futures_util::Stream for WebSocketStreamProxy {
    type Item = tokio_tungstenite_wasm::Result<tokio_tungstenite_wasm::Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let inner = self.inner.get_mut();
        futures::pin_mut!(inner);

        inner.poll_next(cx)
    }
}

impl futures_util::Sink<tokio_tungstenite_wasm::Message> for WebSocketStreamProxy {
    type Error = tokio_tungstenite_wasm::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let inner = self.inner.get_mut();
        futures::pin_mut!(inner);

        inner.poll_ready(cx)
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: tokio_tungstenite_wasm::Message,
    ) -> Result<(), Self::Error> {
        let inner = self.inner.get_mut();
        futures::pin_mut!(inner);

        inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Ok(()).into()
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let inner = self.inner.get_mut();
        futures::pin_mut!(inner);

        inner.poll_close(cx)
    }
}

async fn wasm_client_connect(
    request_url: String,
) -> Result<WebSocketStreamProxy, tokio_tungstenite_wasm::Error> {
    let (result_sender, result_receiver) = async_channel::bounded(1usize);

    wasm_bindgen_futures::spawn_local(async move {
        let result = tokio_tungstenite_wasm::connect(request_url.as_str()).await;
        result_sender
            .send_blocking(result.map(|websocket| fragile::Fragile::new(websocket)))
            .unwrap();
    });

    let websocket = result_receiver
        .recv()
        .await
        .unwrap_or(Err(tokio_tungstenite_wasm::Error::ConnectionClosed))?;

    Ok(WebSocketStreamProxy { inner: websocket })
}
