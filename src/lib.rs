mod client;
mod server;
mod websocket;

pub use websocket::Message;
pub use websocket::CloseCode;

pub use client::connect;
pub use client::Client;
pub use client::ClientHandle;

pub use server::run;
pub use server::Server;
pub use server::ServerHandle;
pub use server::Session;
pub use server::SessionHandle;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;