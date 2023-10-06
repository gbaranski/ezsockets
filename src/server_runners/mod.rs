cfg_if::cfg_if! {
    if #[cfg(feature = "axum")] {
        pub mod axum;
        pub mod axum_tungstenite;
    }
}

#[cfg(feature = "tokio-tungstenite")]
pub mod tungstenite;

#[cfg(feature = "tungstenite_common")]
pub mod tungstenite_common;
