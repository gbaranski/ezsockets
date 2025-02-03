//! The `axum` feature must be enabled in order to use this module.
//!
//! ```no_run
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
//! #   async fn on_text(&mut self, text: ezsockets::Utf8Bytes) -> Result<(), ezsockets::Error> { unimplemented!() }
//! #   async fn on_binary(&mut self, bytes: ezsockets::Bytes) -> Result<(), ezsockets::Error> { unimplemented!() }
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
//!    # async fn on_connect(&mut self, socket: ezsockets::Socket, request: ezsockets::Request, address: std::net::SocketAddr) -> Result<ezsockets::Session<u16, ()>, Option<ezsockets::CloseFrame>> { unimplemented!() }
//!    # async fn on_disconnect(&mut self, id: <Self::Session as ezsockets::SessionExt>::ID, reason: Result<Option<ezsockets::CloseFrame>, ezsockets::Error>) -> Result<(), ezsockets::Error> { unimplemented!() }
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
//!         let listener = tokio::net::TcpListener::bind(address).await.unwrap();
//!          axum::serve(
//!                 listener,
//!                 app.into_make_service_with_connect_info::<SocketAddr>(),
//!             )
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

use crate::server_runners::axum_tungstenite::rejection::*;
use crate::server_runners::axum_tungstenite::WebSocketUpgrade;
use crate::socket::SocketConfig;
use crate::Server;
use crate::ServerExt;
use crate::Socket;
use axum::extract::ConnectInfo;
use axum::extract::FromRequestParts;
use axum::response::Response;
use enfync::TryAdopt;
use http::request::Parts;
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
    ws: WebSocketUpgrade,
    address: SocketAddr,
    request: crate::Request,
}

impl Upgrade {
    pub fn address(&self) -> &SocketAddr {
        &self.address
    }

    pub fn request(&self) -> &crate::Request {
        &self.request
    }
}

impl<S> FromRequestParts<S> for Upgrade
where
    S: Send + Sync,
{
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ConnectInfo(address) = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .expect("Axum Server must be created with `axum::Router::into_make_service_with_connect_info::<SocketAddr, _>()`")
            .to_owned();

        let mut pure_req = crate::Request::builder()
            .method(parts.method.clone())
            .uri(parts.uri.clone())
            .version(parts.version);
        for (k, v) in parts.headers.iter() {
            pure_req = pure_req.header(k, v);
        }
        let Ok(pure_req) = pure_req.body(()) else {
            return Err(InvalidConnectionHeader {}.into());
        };

        Ok(Self {
            ws: WebSocketUpgrade::from_request_parts(parts, state).await?,
            address,
            request: pure_req,
        })
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
        self.on_upgrade_with_config(server, SocketConfig::default())
    }

    /// Finalize upgrading the connection.
    ///
    /// When using `WebSocketUpgrade`, the response produced by this method
    /// should be returned from the handler. See the [module docs](self) for an
    /// example.
    pub fn on_upgrade_with_config<E: ServerExt + 'static>(
        self,
        server: Server<E>,
        socket_config: SocketConfig,
    ) -> Response {
        self.ws.on_upgrade(move |socket| async move {
            let handle = enfync::builtin::native::TokioHandle::try_adopt()
                .expect("axum server runner only works in a tokio runtime");
            let socket = Socket::new(socket, socket_config, handle);
            server.accept(socket, self.request, self.address);
        })
    }
}
