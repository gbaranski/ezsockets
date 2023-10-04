//! Creating a WebSocket server or a client in Rust can be troublesome. This crate facilitates this process by providing:
//! - High-level abstraction of WebSocket, handling Ping/Pong from both Client and Server.
//! - Traits to allow declarative and event-based programming.
//! - Automatic reconnection of WebSocket Client.
//!
//! Refer to [`client`] or [`server`] module for detailed implementation guides.

mod server_runners;
mod socket;

pub use server_runners::*;

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
        pub mod client;

        pub use client::connect;
        pub use client::ClientConfig;
        pub use client::ClientExt;
        pub use client::Client;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "server")] {
        pub mod server;
        pub mod session;

        pub use server::Server;
        pub use server::ServerExt;

        pub use session::Session;
        pub use session::SessionExt;
    }
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Request = http::Request<()>;
