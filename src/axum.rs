use std::net::SocketAddr;

use crate::RawMessage;
use crate::Socket;
use async_trait::async_trait;
use axum_crate::extract::{ws, ConnectInfo};
use axum_crate::extract::ws::rejection::*;
use axum_crate::extract::{FromRequest, RequestParts};
use axum_crate::response::Response;
use futures::Future;
use tokio_tungstenite::tungstenite;

/// Extractor for establishing WebSocket connections.
///
/// Note: This extractor requires the request method to be `GET` so it should
/// always be used with [`get`](crate::routing::get). Requests with other methods will be
/// rejected.
///
/// See the [module docs](self) for an example.
#[derive(Debug)]
pub struct Upgrade {
    ws: ws::WebSocketUpgrade,
    address: SocketAddr,
}

#[async_trait]
impl<B> FromRequest<B> for Upgrade
where
    B: Send,
{
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let ConnectInfo(address) = req
            .extensions()
            .unwrap()
            .get::<ConnectInfo<SocketAddr>>()
            .unwrap()
            .to_owned();
        Ok(Self {
            ws: ws::WebSocketUpgrade::from_request(req).await?,
            address,
        })
    }
}

fn into_tungstenite(message: ws::Message) -> tungstenite::Message {
    match message {
        ws::Message::Text(text) => tungstenite::Message::Text(text),
        ws::Message::Binary(binary) => tungstenite::Message::Binary(binary),
        ws::Message::Ping(ping) => tungstenite::Message::Ping(ping),
        ws::Message::Pong(pong) => tungstenite::Message::Pong(pong),
        ws::Message::Close(Some(close)) => {
            tungstenite::Message::Close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::from(close.code),
                reason: close.reason,
            }))
        }
        ws::Message::Close(None) => tungstenite::Message::Close(None),
    }
}

fn from_tungstenite(message: tungstenite::Message) -> Option<ws::Message> {
    match message {
        tungstenite::Message::Text(text) => Some(ws::Message::Text(text)),
        tungstenite::Message::Binary(binary) => Some(ws::Message::Binary(binary)),
        tungstenite::Message::Ping(ping) => Some(ws::Message::Ping(ping)),
        tungstenite::Message::Pong(pong) => Some(ws::Message::Pong(pong)),
        tungstenite::Message::Close(Some(close)) => {
            Some(ws::Message::Close(Some(ws::CloseFrame {
                code: close.code.into(),
                reason: close.reason,
            })))
        }
        tungstenite::Message::Close(None) => Some(ws::Message::Close(None)),
        // we can ignore `Frame` frames as recommended by the tungstenite maintainers
        // https://github.com/snapview/tungstenite-rs/issues/268
        tungstenite::Message::Frame(_) => None,
    }
}

impl From<ws::Message> for RawMessage {
    fn from(message: ws::Message) -> Self {
        Self::from(into_tungstenite(message))
    }
}

impl From<RawMessage> for ws::Message {
    fn from(message: RawMessage) -> Self {
        Self::from(from_tungstenite(message.into()).unwrap())
    }
}

impl Upgrade {
    /// Finalize upgrading the connection and call the provided callback with
    /// the stream.
    ///
    /// When using `WebSocketUpgrade`, the response produced by this method
    /// should be returned from the handler. See the [module docs](self) for an
    /// example.
    pub fn on_upgrade<F, Fut>(self, callback: F) -> Response
    where
        F: FnOnce(Socket, SocketAddr) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.ws.on_upgrade(move |socket| async move {
            let socket = Socket::new(socket);
            callback(socket, self.address).await;
        })
    }
}
