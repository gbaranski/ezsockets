//! Creating a WebSocket server or a client in Rust can be troublesome. This crate facilitates this process by providing:
//! - High-level abstraction of WebSocket, handling Ping/Pong from both Client and Server.
//! - Traits to allow declarative and event-based programming.
//! - Automatic reconnection of WebSocket Client.
//!
//! Refer to [`client`] or [`server`] module for detailed implementation guides.

#![cfg_attr(docsrs, feature(doc_cfg))]

mod socket;
mod tungstenite_common;

pub use socket::CloseCode;
pub use socket::CloseFrame;
pub use socket::Message;
pub use socket::MessageSignal;
pub use socket::MessageStatus;
pub use socket::RawMessage;
pub use socket::Sink;
pub use socket::Socket;
pub use socket::SocketConfig;
pub use socket::Stream;

cfg_if::cfg_if! {
    if #[cfg(feature = "client")] {
        #[cfg_attr(docsrs, doc(cfg(feature = "client")))]
        pub mod client;
        #[cfg_attr(docsrs, doc(cfg(feature = "client")))]
        mod client_connectors;

        pub use client_connectors::*;

        #[cfg(feature = "native_client")]
        pub use client::connect;

        pub use client::connect_with;
        pub use client::ClientConfig;
        pub use client::ClientExt;
        pub use client::Client;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "server")] {
        #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
        pub mod server;
        #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
        pub mod session;
        #[cfg(any(feature = "axum", feature = "tungstenite"))]
        #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
        pub mod server_runners;

        #[cfg(any(feature = "axum", feature = "tungstenite"))]
        pub use server_runners::*;

        pub use server::Server;
        pub use server::ServerExt;

        pub use session::Session;
        pub use session::SessionExt;
    }
}

pub use tokio_tungstenite_wasm::Error as WSError;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Request = http::Request<()>;
