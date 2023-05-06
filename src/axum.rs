//! `axum` feature must be enabled in order to use this module.
//!
//! ```no_run
//! # use axum_crate as axum;
//! use async_trait::async_trait;
//! use axum::routing::get;
//! use axum::Extension;
//! use axum::response::IntoResponse;
//! use std::net::SocketAddr;
//! use ezsockets::axum::Upgrade;
//!
//! # struct MySession {}
//! # #[async_trait]
//! # impl ezsockets::SessionExt for MySession {
//! #   type ID = u16;
//! #   type Call = ();
//! #   fn id(&self) -> &Self::ID { unimplemented!() }
//! #   async fn on_text(&mut self, text: String) -> Result<(), ezsockets::Error> { unimplemented!() }
//! #   async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> { unimplemented!() }
//! #   async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> { unimplemented!() }
//! # }
//! #
//! struct EchoServer {}
//!
//! #[async_trait]
//! impl ezsockets::ServerExt for EchoServer {
//!     // ...
//!    # type Session = MySession;
//!    # type Call = ();
//!    # async fn on_connect(&mut self, socket: ezsockets::Socket, request: ezsockets::Request, address: std::net::SocketAddr) -> Result<ezsockets::Session<u16, ()>, ezsockets::Error> { unimplemented!() }
//!    # async fn on_disconnect(&mut self, id: <Self::Session as ezsockets::SessionExt>::ID) -> Result<(), ezsockets::Error> { unimplemented!() }
//!    # async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> { unimplemented!() }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let (server, _) = ezsockets::Server::create(|_| EchoServer {});
//!     let app = axum::Router::new()
//!         .route("/websocket", get(websocket_handler))
//!         .layer(Extension(server.clone()));
//!
//!     let address = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));
//!
//!     tokio::spawn(async move {
//!         tracing::debug!("listening on {}", address);
//!         axum::Server::bind(&address)
//!             .serve(app.into_make_service_with_connect_info::<SocketAddr>())
//!             .await
//!             .unwrap();
//!     });
//!
//! }
//!
//! async fn websocket_handler(
//!     Extension(server): Extension<ezsockets::Server<EchoServer>>,
//!     ezsocket: Upgrade,
//! ) -> impl IntoResponse {
//!     ezsocket.on_upgrade(server)
//! }
//! ```

use axum_crate as axum;

use crate::CloseCode;
use crate::CloseFrame;
use crate::Message;
use crate::Server;
use crate::ServerExt;
use crate::Socket;
use async_trait::async_trait;
use axum::extract::ws;
use axum::extract::ws::rejection::*;
use axum::extract::ConnectInfo;
use axum::extract::FromRequest;
use axum::response::Response;
use std::net::SocketAddr;

/// Extractor for establishing WebSocket connections.
///
/// Note: This extractor requires the request method to be `GET` so it should
/// always be used with [`get`](axum::routing::get). Requests with other methods will be
/// rejected.
///
/// See the [module docs](self) for an example.
#[derive(Debug)]
pub struct Upgrade {
    ws: ws::WebSocketUpgrade,
    address: SocketAddr,
    request: crate::Request,
}

#[async_trait]
impl<S, B> FromRequest<S, B> for Upgrade
where
    S: Send + Sync,
    B: Send + 'static,
{
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request(req: http::Request<B>, state: &S) -> Result<Self, Self::Rejection> {
        let ConnectInfo(address) = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .expect("Axum Server must be created with `axum::Router::into_make_service_with_connect_info::<SocketAddr, _>()`")
            .to_owned();

        let mut pure_req = crate::Request::builder()
            .method(req.method())
            .uri(req.uri())
            .version(req.version());
        for (k, v) in req.headers() {
            pure_req = pure_req.header(k, v);
        }
        let pure_req = pure_req.body(()).unwrap();

        Ok(Self {
            ws: ws::WebSocketUpgrade::from_request(req, state).await?,
            address,
            request: pure_req,
        })
    }
}

impl From<ws::Message> for Message {
    fn from(message: ws::Message) -> Self {
        match message {
            ws::Message::Text(text) => Message::Text(text),
            ws::Message::Binary(binary) => Message::Binary(binary),
            ws::Message::Ping(ping) => Message::Ping(ping),
            ws::Message::Pong(pong) => Message::Pong(pong),
            ws::Message::Close(Some(close)) => Message::Close(Some(CloseFrame {
                code: CloseCode::try_from(close.code).unwrap(),
                reason: close.reason.into(),
            })),
            ws::Message::Close(None) => Message::Close(None),
        }
    }
}

impl From<Message> for ws::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => ws::Message::Text(text),
            Message::Binary(binary) => ws::Message::Binary(binary),
            Message::Ping(ping) => ws::Message::Ping(ping),
            Message::Pong(pong) => ws::Message::Pong(pong),
            Message::Close(Some(close)) => ws::Message::Close(Some(ws::CloseFrame {
                code: close.code.into(),
                reason: close.reason.into(),
            })),
            Message::Close(None) => ws::Message::Close(None),
        }
    }
}

impl Upgrade {
    /// Finalize upgrading the connection and call the provided callback with
    /// the stream.
    ///
    /// When using `WebSocketUpgrade`, the response produced by this method
    /// should be returned from the handler. See the [module docs](self) for an
    /// example.
    pub fn on_upgrade<E: ServerExt + 'static>(self, server: Server<E>) -> Response {
        self.ws.on_upgrade(move |socket| async move {
            let socket = Socket::new(socket, Default::default()); // TODO: Make it really configurable via Extensions
            server.accept(socket, self.request, self.address).await;
        })
    }
}
