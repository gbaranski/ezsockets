//! `tungstenite` feature must be enabled in order to use this module.
//!
//! ```rust
//! struct EchoServer {}
//!
//! #[async_trait]
//! impl ezsockets::ServerExt for EchoServer {
//!     // ...
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let (server, _) = ezsockets::Server::create(|_| EchoServer {});
//!     ezsockets::tungstenite::run(server, "127.0.0.1:8080", |_socket| async move { Ok(()) })
//!         .await
//!         .unwrap();
//! }
//! ```
//!

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
        use futures::Future;

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
            loop {
                let (socket, address) = listener.accept().await?;
                let socket = tokio_tungstenite::accept_async(socket).await?;
                let mut socket = Socket::new(socket, socket::Config::default());
                let args = get_args(&mut socket).await?;
                server.accept(socket, address, args).await;
            }
        }

        pub async fn run_on<E, GetArgsFut>(
            server: Server<E>,
            listener: TcpListener,
            get_args: impl Fn(&mut Socket) -> GetArgsFut
        ) -> Result<(), Error>
        where
            E: ServerExt + 'static,
            GetArgsFut: Future<Output = Result<<E::Session as SessionExt>::Args, Error>>
        {
            loop {
                let (socket, address) = listener.accept().await?;
                let socket = tokio_tungstenite::accept_async(socket).await?;
                let mut socket = Socket::new(socket, socket::Config::default());
                let args = get_args(&mut socket).await?;
                server.accept(socket, address, args).await;
            }
        }
    }
}
