cfg_if::cfg_if! {
    if #[cfg(feature = "axum")] {
        #[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
        pub mod axum;
        #[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
        pub mod axum_tungstenite;
    }
}

#[cfg(feature = "tungstenite")]
#[cfg_attr(docsrs, doc(cfg(feature = "tungstenite")))]
pub mod tungstenite;
