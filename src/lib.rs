mod client;
mod server;
mod session;
mod websocket;

#[cfg(feature = "axum")]
pub mod axum;

pub use websocket::CloseCode;
pub use websocket::CloseFrame;
pub use websocket::Message;
pub use websocket::WebSocket;

pub use client::connect;
pub use client::ClientConfig;
pub use client::Client;
pub use client::ClientHandle;

pub use server::run;
pub use server::Server;
pub use server::ServerHandle;

pub use session::Session;
pub use session::SessionHandle;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;