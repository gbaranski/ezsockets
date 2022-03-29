mod socket;

pub use socket::CloseCode;
pub use socket::CloseFrame;
pub use socket::Message;
pub use socket::RawMessage;
pub use socket::Socket;
pub use socket::Stream;
pub use socket::Sink;

#[cfg(feature = "server-axum")]
pub mod axum;

#[cfg(feature = "tokio-tungstenite")]
pub mod tungstenite;

cfg_if::cfg_if! {
    if #[cfg(feature = "client")] {
        mod client;

        pub use client::connect;
        pub use client::ClientConfig;
        pub use client::ClientExt;
        pub use client::Client;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "server")] {
        mod server;
        mod session;

        pub use server::Server;
        pub use server::ServerExt;

        pub use session::Session;
        pub use session::SessionExt;
    }
}

use std::sync::Arc;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(Arc<std::io::Error>),

    #[cfg(feature = "axum")]
    #[error("axum: {0}")]
    Axum(Arc<::axum::Error>),

    #[cfg(feature = "tokio-tungstenite")]
    #[error("tungstenite: {0}")]
    Tungstenite(Arc<tokio_tungstenite::tungstenite::Error>),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(Arc::new(error))
    }
}

#[cfg(feature = "axum")]
impl From<::axum::Error> for Error {
    fn from(error: ::axum::Error) -> Self {
        Self::Axum(Arc::new(error))
    }
}

#[cfg(feature = "tokio-tungstenite")]
impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Tungstenite(Arc::new(error))
    }
}