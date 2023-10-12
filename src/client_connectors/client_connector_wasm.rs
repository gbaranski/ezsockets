use crate::client::{ClientConfig, ClientConnector};

/// Implementation of [`ClientConnector`] for tokio runtimes.
#[derive(Clone)]
pub struct ClientConnectorWasm {
    handle: enfync::builtin::wasm::WASMHandle,
}

impl Default for ClientConnectorWasm {
    fn default() -> Self {
        let handle = enfync::builtin::wasm::WASMHandle::default();
        Self { handle }
    }
}

#[async_trait::async_trait]
impl ClientConnector for ClientConnectorWasm {
    type Handle = enfync::builtin::wasm::WASMHandle;
    type Message = tokio_tungstenite_wasm::Message;
    type WSError = tokio_tungstenite_wasm::Error;
    type Socket = tokio_tungstenite_wasm::WebSocketStream;
    type ConnectError = tokio_tungstenite_wasm::Error;

    /// Get the connector's runtime handle.
    fn handle(&self) -> Self::Handle {
        self.handle.clone()
    }

    /// Connect to a websocket server.
    ///
    /// Returns `Err` if the request is invalid.
    ///
    /// Panics if any headers were added to the client config. Websockets on browser does not support
    /// additional headers (use [`ClientConfig::query_parameter()`] instead).
    async fn connect(&self, config: &ClientConfig) -> Result<Self::Socket, Self::ConnectError> {
        if config.headers().len() > 0 {
            panic!("client may not submit HTTP headers in WASM connection requests");
        }
        let request_url = config.connect_url();
        let socket = tokio_tungstenite_wasm::connect(request_url).await?;
        Ok(socket)
    }
}
