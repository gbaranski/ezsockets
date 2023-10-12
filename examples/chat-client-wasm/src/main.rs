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

    async fn on_text(&mut self, text: String) -> Result<(), ezsockets::Error> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> {
        let () = call;
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(main)]
async fn main() -> Result<(), wasm_bindgen::JsValue> {
    // setup tracing
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();

    // make client
    let config = ClientConfig::new("ws://localhost:8080/websocket").socket_config(SocketConfig {
        heartbeat: Duration::from_secs(5),
        timeout: Duration::from_secs(10),
        heartbeat_ping_msg_fn: Arc::new(|_t: Duration| RawMessage::Binary("ping".into())),
    });
    let (client, mut handle) = ezsockets::connect_with(
        |_client| Client {},
        config,
        ezsockets::ClientConnectorWasm::default(),
    );

    // collect inputs: todo

    // keep main alive until it is manually terminated
    handle.extract().await.unwrap().unwrap();

    Ok(())
}
