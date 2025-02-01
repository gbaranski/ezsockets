#![allow(unused_imports)]

use async_trait::async_trait;
use ezsockets::{ClientConfig, RawMessage, SocketConfig};
use std::io::BufRead;
use std::sync::Arc;
use std::time::Duration;

struct Client {}

#[async_trait]
impl ezsockets::ClientExt for Client {
    type Call = ();

    async fn on_text(&mut self, text: ezsockets::Utf8Bytes) -> Result<(), ezsockets::Error> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn on_binary(&mut self, bytes: ezsockets::Bytes) -> Result<(), ezsockets::Error> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> {
        let () = call;
        Ok(())
    }
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(main)]
async fn main() -> Result<(), wasm_bindgen::JsValue> {
    // setup tracing
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();

    // make client
    // - note: we use a hacky custom Ping/Pong protocol to keep the client alive (see the 'chat-server' example
    //         for the Ping side via SessionExt::on_binary())
    let config = ClientConfig::new("ws://localhost:8080/websocket").socket_config(SocketConfig {
        heartbeat_ping_msg_fn: Arc::new(|_t: Duration| RawMessage::Binary("ping".into())),
        ..Default::default()
    });
    let (_client, mut handle) = ezsockets::connect_with(
        |_client| Client {},
        config,
        ezsockets::ClientConnectorWasm::default(),
    );

    // collect inputs: todo

    // keep main alive until it is manually terminated
    handle.extract().await.unwrap().unwrap();

    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    // need per-package targets https://github.com/rust-lang/cargo/issues/9406
    let _ = Client {};
    unreachable!()
}
