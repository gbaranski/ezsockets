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

use crate::server_runners::axum_tungstenite::rejection::*;
use crate::server_runners::axum_tungstenite::WebSocketUpgrade;
use crate::socket::SocketConfig;
use crate::Server;
use crate::ServerExt;
use crate::Socket;
use async_trait::async_trait;
use axum::extract::ConnectInfo;
use axum::extract::FromRequest;
use axum::response::Response;
use enfync::TryAdopt;
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
        let Ok(pure_req) = pure_req.body(()) else {
            return Err(InvalidConnectionHeader {}.into());
        };

        Ok(Self {
            ws: WebSocketUpgrade::from_request(req, state).await?,
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

    /// Finalize upgrading the connection and call the provided callback with
    /// the stream.
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
