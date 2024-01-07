cfg_if::cfg_if! {
    if #[cfg(all(feature = "native_client", not(target_family = "wasm")))] {
        #[cfg_attr(docsrs, doc(cfg(all(feature = "native_client", not(target_family = "wasm")))))]
        mod client_connector_tokio;
        pub use client_connector_tokio::*;
    }
}

cfg_if::cfg_if! {
    if #[cfg(all(feature = "wasm_client", target_family = "wasm"))] {
        #[cfg_attr(docsrs, doc(cfg(all(feature = "wasm_client", target_family = "wasm"))))]
        mod client_connector_wasm;
        pub use client_connector_wasm::*;
    }
}
