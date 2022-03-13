mod client;
mod server;
mod session;
mod socket;

#[cfg(feature = "axum")]
pub mod axum;

#[cfg(feature = "tungstenite")]
pub mod tungstenite;

pub use socket::RawMessage;
pub use socket::Message;
pub use socket::Socket;
pub use socket::CloseCode;
pub use socket::CloseFrame;

pub use client::connect;
pub use client::ClientConfig;
pub use client::Client;
pub use client::ClientHandle;

pub use server::ServerExt;
pub use server::Server;

pub use session::Session;
pub use session::SessionHandle;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;