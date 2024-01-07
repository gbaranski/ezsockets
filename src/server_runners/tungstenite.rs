//! The `tungstenite` feature must be enabled in order to use this module.
//!
//! ```no_run
//! # use async_trait::async_trait;
//! # struct MySession {}
//! # #[async_trait::async_trait]
//! # impl ezsockets::SessionExt for MySession {
//! #   type ID = u16;
//! #   type Call = ();
//! #   fn id(&self) -> &Self::ID { unimplemented!() }
//! #   async fn on_text(&mut self, text: String) -> Result<(), ezsockets::Error> { unimplemented!() }
//! #   async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> { unimplemented!() }
//! #   async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> { unimplemented!() }
//! # }
//! struct MyServer {}
//!
//! #[async_trait]
//! impl ezsockets::ServerExt for MyServer {
//!    // ...
//!    # type Session = MySession;
//!    # type Call = ();
//!    # async fn on_connect(&mut self, socket: ezsockets::Socket, request: ezsockets::Request, address: std::net::SocketAddr) -> Result<ezsockets::Session<u16, ()>, Option<ezsockets::CloseFrame>> { unimplemented!() }
//!    # async fn on_disconnect(&mut self, id: <Self::Session as ezsockets::SessionExt>::ID, reason: Result<Option<ezsockets::CloseFrame>, ezsockets::Error>) -> Result<(), ezsockets::Error> { unimplemented!() }
//!    # async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> { unimplemented!() }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let (server, _) = ezsockets::Server::create(|_| MyServer {});
//!     ezsockets::tungstenite::run(server, "127.0.0.1:8080").await.unwrap();
//! }
//! ```

use crate::tungstenite::tungstenite::handshake::server::ErrorResponse;
use crate::Error;
use crate::Request;
use crate::Server;
use crate::ServerExt;
use crate::Socket;
use crate::SocketConfig;

use enfync::TryAdopt;

use tokio_tungstenite::tungstenite;

use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::net::ToSocketAddrs;

pub enum Acceptor {
    Plain,

    #[cfg(feature = "native-tls")]
    #[cfg_attr(docsrs, doc(cfg(feature = "native-tls")))]
    NativeTls(tokio_native_tls::TlsAcceptor),

    #[cfg(feature = "rustls")]
    #[cfg_attr(docsrs, doc(cfg(feature = "rustls")))]
    Rustls(tokio_rustls::TlsAcceptor),
}

impl Acceptor {
    async fn accept(
        &self,
        stream: TcpStream,
        handle: &enfync::builtin::native::TokioHandle,
    ) -> Result<(Socket, Request), Error> {
        let mut req0 = None;
        let callback = |req: &http::Request<()>,
                        resp: http::Response<()>|
         -> Result<http::Response<()>, ErrorResponse> {
            let mut req1 = Request::builder()
                .method(req.method().clone())
                .uri(req.uri().clone())
                .version(req.version());
            for (k, v) in req.headers() {
                req1 = req1.header(k, v);
            }
            let Ok(body) = req1.body(()) else {
                return Err(ErrorResponse::default());
            };
            req0 = Some(body);

            Ok(resp)
        };
        let socket = match self {
            Acceptor::Plain => {
                let socket = tokio_tungstenite::accept_hdr_async(stream, callback).await?;
                Socket::new(socket, SocketConfig::default(), handle.clone())
            }
            #[cfg(feature = "native-tls")]
            Acceptor::NativeTls(acceptor) => {
                let tls_stream = acceptor.accept(stream).await?;
                let socket = tokio_tungstenite::accept_hdr_async(tls_stream, callback).await?;
                Socket::new(socket, SocketConfig::default(), handle.clone())
            }
            #[cfg(feature = "rustls")]
            Acceptor::Rustls(acceptor) => {
                let tls_stream = acceptor.accept(stream).await?;
                let socket = tokio_tungstenite::accept_hdr_async(tls_stream, callback).await?;
                Socket::new(socket, SocketConfig::default(), handle.clone())
            }
        };
        let Some(req_body) = req0 else {
            return Err("invalid request body".into());
        };
        Ok((socket, req_body))
    }
}

async fn run_acceptor<E>(
    server: Server<E>,
    listener: TcpListener,
    acceptor: Acceptor,
    handle: &enfync::builtin::native::TokioHandle,
) -> Result<(), Error>
where
    E: ServerExt + 'static,
{
    loop {
        // TODO: Find a better way without those stupid matches
        let (stream, address) = match listener.accept().await {
            Ok(stream) => stream,
            Err(err) => {
                tracing::warn!("failed to accept tcp connection: {:?}", err);
                continue;
            }
        };
        let (socket, request) = match acceptor.accept(stream, handle).await {
            Ok(socket) => socket,
            Err(err) => {
                tracing::warn!(%address, "failed to accept websocket connection: {:?}", err);
                continue;
            }
        };
        server.accept(socket, request, address);
    }
}

// Run the server
pub async fn run<E, A>(server: Server<E>, address: A) -> Result<(), Error>
where
    E: ServerExt + 'static,
    A: ToSocketAddrs,
{
    let listener = TcpListener::bind(address).await?;
    run_on(server, listener, Acceptor::Plain).await
}

/// Run the server on custom `Listener` and `Acceptor`
/// For default acceptor use `Acceptor::plain`
pub async fn run_on<E>(
    server: Server<E>,
    listener: TcpListener,
    acceptor: Acceptor,
) -> Result<(), Error>
where
    E: ServerExt + 'static,
{
    let handle = enfync::builtin::native::TokioHandle::try_adopt()
        .expect("tungstenite server runner only works in a tokio runtime");
    run_acceptor(server, listener, acceptor, &handle).await
}
