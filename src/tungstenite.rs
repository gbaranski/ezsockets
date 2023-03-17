//! `tungstenite` feature must be enabled in order to use this module.
//!
//! ```no_run
//! # use async_trait::async_trait;
//! # struct MySession {}
//! # #[async_trait::async_trait]
//! # impl ezsockets::SessionExt for MySession {
//! #   type ID = u16;
//! #   type Args = ();
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
//!    # async fn on_connect(&mut self, socket: ezsockets::Socket, address: std::net::SocketAddr, _args: ()) -> Result<ezsockets::Session<u16, ()>, ezsockets::Error> { unimplemented!() }
//!    # async fn on_disconnect(&mut self, id: <Self::Session as ezsockets::SessionExt>::ID) -> Result<(), ezsockets::Error> { unimplemented!() }
//!    # async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> { unimplemented!() }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let (server, _) = ezsockets::Server::create(|_| MyServer {});
//!     ezsockets::tungstenite::run(server, "127.0.0.1:8080", |_socket| async move { Ok(()) }).await.unwrap();
//! }
//! ```

use crate::socket::RawMessage;
use crate::CloseCode;
use crate::CloseFrame;
use crate::Message;
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
        use crate::socket;
        use crate::ServerExt;
        use crate::SessionExt;

        use tokio::net::TcpListener;
        use tokio::net::ToSocketAddrs;
        use tokio::net::TcpStream;
        use futures::Future;

        pub enum Acceptor {
            Plain,
            #[cfg(feature = "native-tls")]
            NativeTls(tokio_native_tls::TlsAcceptor),
            #[cfg(feature = "rustls")]
            Rustls(tokio_rustls::TlsAcceptor),
        }

        impl Acceptor {
            async fn accept(&self, stream: TcpStream) -> Result<Socket, Error> {
                let socket = match self {
                    Acceptor::Plain => {
                        let socket = tokio_tungstenite::accept_async(stream).await?;
                        Socket::new(socket, socket::Config::default())
                    }
                    #[cfg(feature = "native-tls")]
                    Acceptor::NativeTls(acceptor) => {
                        let tls_stream = acceptor.accept(stream).await?;
                        let socket = tokio_tungstenite::accept_async(tls_stream).await?;
                        Socket::new(socket, socket::Config::default())
                    }
                    #[cfg(feature = "rustls")]
                    Acceptor::Rustls(acceptor) => {
                        let tls_stream = acceptor.accept(stream).await?;
                        let socket = tokio_tungstenite::accept_async(tls_stream).await?;
                        Socket::new(socket, socket::Config::default())
                    }
                };
                Ok(socket)
            }
        }

        async fn run_acceptor<E, GetArgsFut>(
            server: Server<E>,
            listener: TcpListener,
            acceptor: Acceptor,
            get_args: impl Fn(&mut Socket) -> GetArgsFut
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static,
            GetArgsFut: Future<Output = Result<<E::Session as SessionExt>::Args, Error>>
        {
            loop {
                // TODO: Find a better way without those stupid matches
                let (stream, address) = match listener.accept().await {
                    Ok(stream) => stream,
                    Err(err) => {
                        tracing::error!("failed to accept tcp connection: {err}");
                        continue;
                    },
                };
                let mut socket = match acceptor.accept(stream).await {
                    Ok(socket) => socket,
                    Err(err) => {
                        tracing::error!(%address, "failed to accept websocket connection: {err}");
                        continue;
                    }
                };
                let args = match get_args(&mut socket).await {
                    Ok(socket) => socket,
                    Err(err) => {
                        tracing::error!(%address, "failed to get session args: {err}");
                        continue;
                    }
                };
                server.accept(socket, address, args).await;
            }
        }

        // Run the server
        pub async fn run<E, A, GetArgsFut>(
            server: Server<E>,
            address: A,
            get_args: impl Fn(&mut Socket) -> GetArgsFut
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static,
            A: ToSocketAddrs,
            GetArgsFut: Future<Output = Result<<E::Session as SessionExt>::Args, Error>>
        {
            let listener = TcpListener::bind(address).await?;
            run_acceptor(server, listener, Acceptor::Plain, get_args).await
        }

        /// Run the server on custom `Listener` and `Acceptor`
        /// For default acceptor use `Acceptor::plain`
        pub async fn run_on<E, GetArgsFut>(
            server: Server<E>,
            listener: TcpListener,
            acceptor: Acceptor,
            get_args: impl Fn(&mut Socket) -> GetArgsFut
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static,
            GetArgsFut: Future<Output = Result<<E::Session as SessionExt>::Args, Error>>
        {
            run_acceptor(server, listener, acceptor, get_args).await
        }
    }
}
