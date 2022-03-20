mod socket;

pub use socket::CloseCode;
pub use socket::CloseFrame;
pub use socket::Message;
pub use socket::RawMessage;
pub use socket::Socket;

#[cfg(feature = "server-axum")]
pub mod axum;

#[cfg(feature = "tokio-tungstenite")]
pub mod tungstenite;

cfg_if::cfg_if! {
    if #[cfg(feature = "client")] {
        mod client;

        pub use client::connect;
        pub use client::ClientConfig;
        pub use client::Client;
        pub use client::ClientHandle;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "server")] {
        mod server;
        mod session;

        pub use server::ServerExt;
        pub use server::Server;

        pub use session::Session;
        pub use session::SessionHandle;
    }
}

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
