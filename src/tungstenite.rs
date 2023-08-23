//! `tungstenite` feature must be enabled in order to use this module.
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

use crate::socket::{RawMessage, SocketConfig};
use crate::tungstenite::tungstenite::handshake::server::ErrorResponse;
use crate::CloseCode;
use crate::CloseFrame;
use crate::Message;
use crate::Request;
use tokio_tungstenite::tungstenite;
use tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;

impl<'t> From<tungstenite::protocol::CloseFrame<'t>> for CloseFrame {
    fn from(frame: tungstenite::protocol::CloseFrame) -> Self {
        Self {
            code: frame.code.into(),
            reason: frame.reason.into(),
        }
    }
}

impl<'t> From<CloseFrame> for tungstenite::protocol::CloseFrame<'t> {
    fn from(frame: CloseFrame) -> Self {
        Self {
            code: frame.code.into(),
            reason: frame.reason.into(),
        }
    }
}

impl From<CloseCode> for TungsteniteCloseCode {
    fn from(code: CloseCode) -> Self {
        match code {
            CloseCode::Normal => Self::Normal,
            CloseCode::Away => Self::Away,
            CloseCode::Protocol => Self::Protocol,
            CloseCode::Unsupported => Self::Unsupported,
            CloseCode::Status => Self::Status,
            CloseCode::Abnormal => Self::Abnormal,
            CloseCode::Invalid => Self::Invalid,
            CloseCode::Policy => Self::Policy,
            CloseCode::Size => Self::Size,
            CloseCode::Extension => Self::Extension,
            CloseCode::Error => Self::Error,
            CloseCode::Restart => Self::Restart,
            CloseCode::Again => Self::Again,
        }
    }
}

impl From<TungsteniteCloseCode> for CloseCode {
    fn from(code: TungsteniteCloseCode) -> Self {
        match code {
            TungsteniteCloseCode::Normal => Self::Normal,
            TungsteniteCloseCode::Away => Self::Away,
            TungsteniteCloseCode::Protocol => Self::Protocol,
            TungsteniteCloseCode::Unsupported => Self::Unsupported,
            TungsteniteCloseCode::Status => Self::Status,
            TungsteniteCloseCode::Abnormal => Self::Abnormal,
            TungsteniteCloseCode::Invalid => Self::Invalid,
            TungsteniteCloseCode::Policy => Self::Policy,
            TungsteniteCloseCode::Size => Self::Size,
            TungsteniteCloseCode::Extension => Self::Extension,
            TungsteniteCloseCode::Error => Self::Error,
            TungsteniteCloseCode::Restart => Self::Restart,
            TungsteniteCloseCode::Again => Self::Again,
            code => unimplemented!("could not handle close code: {code:?}"),
        }
    }
}

impl From<RawMessage> for tungstenite::Message {
    fn from(message: RawMessage) -> Self {
        match message {
            RawMessage::Text(text) => Self::Text(text),
            RawMessage::Binary(bytes) => Self::Binary(bytes),
            RawMessage::Ping(bytes) => Self::Ping(bytes),
            RawMessage::Pong(bytes) => Self::Pong(bytes),
            RawMessage::Close(frame) => Self::Close(frame.map(CloseFrame::into)),
        }
    }
}

impl From<tungstenite::Message> for RawMessage {
    fn from(message: tungstenite::Message) -> Self {
        match message {
            tungstenite::Message::Text(text) => Self::Text(text),
            tungstenite::Message::Binary(bytes) => Self::Binary(bytes),
            tungstenite::Message::Ping(bytes) => Self::Ping(bytes),
            tungstenite::Message::Pong(bytes) => Self::Pong(bytes),
            tungstenite::Message::Close(frame) => Self::Close(frame.map(CloseFrame::from)),
            tungstenite::Message::Frame(_) => unreachable!(),
        }
    }
}

impl From<Message> for tungstenite::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => tungstenite::Message::Text(text),
            Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
            Message::Close(frame) => tungstenite::Message::Close(frame.map(CloseFrame::into)),
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "server")] {
        use crate::Server;
        use crate::Error;
        use crate::Socket;
        use crate::ServerExt;

        use tokio::net::TcpListener;
        use tokio::net::ToSocketAddrs;
        use tokio::net::TcpStream;

        pub enum Acceptor {
            Plain,
            #[cfg(feature = "native-tls")]
            NativeTls(tokio_native_tls::TlsAcceptor),
            #[cfg(feature = "rustls")]
            Rustls(tokio_rustls::TlsAcceptor),
        }

        impl Acceptor {
            async fn accept(&self, stream: TcpStream) -> Result<(Socket, Request), Error> {
                 let mut req0 = None;
                 let callback = |req: &http::Request<()>, resp: http::Response<()>| -> Result<http::Response<()>, ErrorResponse> {
                    let mut req1 = Request::builder()
                        .method(req.method().clone())
                        .uri(req.uri().clone())
                        .version(req.version());
                    for (k, v) in req.headers() {
                        req1 = req1.header(k, v);
                    }
                    let Ok(body) = req1.body(()) else { return Err(ErrorResponse::default()); };
                    req0 = Some(body);

                    Ok(resp)
                };
                let socket = match self {
                    Acceptor::Plain => {
                        let socket = tokio_tungstenite::accept_hdr_async(stream, callback).await?;
                        Socket::new(socket, SocketConfig::default())
                    }
                    #[cfg(feature = "native-tls")]
                    Acceptor::NativeTls(acceptor) => {
                        let tls_stream = acceptor.accept(stream).await?;
                        let socket = tokio_tungstenite::accept_hdr_async(tls_stream, callback).await?;
                        Socket::new(socket, SocketConfig::default())
                    }
                    #[cfg(feature = "rustls")]
                    Acceptor::Rustls(acceptor) => {
                        let tls_stream = acceptor.accept(stream).await?;
                        let socket = tokio_tungstenite::accept_hdr_async(tls_stream, callback).await?;
                        Socket::new(socket, SocketConfig::default())
                    }
                };
                let Some(req_body) = req0 else { return Err("invalid request body".into()); };
                Ok((socket, req_body))
            }
        }

        async fn run_acceptor<E>(
            server: Server<E>,
            listener: TcpListener,
            acceptor: Acceptor,
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static
        {
            loop {
                // TODO: Find a better way without those stupid matches
                let (stream, address) = match listener.accept().await {
                    Ok(stream) => stream,
                    Err(err) => {
                        tracing::warn!("failed to accept tcp connection: {:?}", err);
                        continue;
                    },
                };
                let (socket, request) = match acceptor.accept(stream).await {
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
        pub async fn run<E, A>(
            server: Server<E>,
            address: A,
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static,
            A: ToSocketAddrs,
        {
            let listener = TcpListener::bind(address).await?;
            run_acceptor(server, listener, Acceptor::Plain).await
        }

        /// Run the server on custom `Listener` and `Acceptor`
        /// For default acceptor use `Acceptor::plain`
        pub async fn run_on<E>(
            server: Server<E>,
            listener: TcpListener,
            acceptor: Acceptor,
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static
        {
            run_acceptor(server, listener, acceptor).await
        }
    }
}
