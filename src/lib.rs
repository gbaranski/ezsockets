mod socket;

pub use socket::CloseCode;
pub use socket::CloseFrame;
pub use socket::Message;
pub use socket::RawMessage;
pub use socket::Sink;
pub use socket::Socket;
pub use socket::Stream;

#[cfg(feature = "axum")]
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
        
        #[cfg(feature = "json")]
        pub use client::JsonClientExt;
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

pub type Error = Box<dyn std::error::Error + Send + Sync>;
