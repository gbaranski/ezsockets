mod client;
mod server;
mod websocket;
mod session;

pub use websocket::CloseCode;
pub use websocket::CloseFrame;
pub use websocket::Message;
pub(crate) use websocket::WebSocketStream;

pub use client::connect;
pub use client::Client;
pub use client::ClientHandle;
pub use client::ClientConfig;

pub use server::run;
pub use server::Server;
pub use server::ServerHandle;

pub use session::Session;
pub use session::SessionHandle;
pub(crate) use session::SessionActor;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
