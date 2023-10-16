use crate::client::{ClientConfig, ClientConnector};
use enfync::TryAdopt;
use tokio_tungstenite::tungstenite;

/// Implementation of [`ClientConnector`] for tokio runtimes.
#[derive(Clone)]
pub struct ClientConnectorTokio {
    handle: enfync::builtin::native::TokioHandle,
}

impl ClientConnectorTokio {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle: handle.into(),
        }
    }
}

impl Default for ClientConnectorTokio {
    fn default() -> Self {
        let handle = enfync::builtin::native::TokioHandle::try_adopt()
            .expect(
                "ClientConnectorTokio::default() only works inside a tokio runtime; use ClientConnectorTokio::new() instead"
            );
        Self { handle }
    }
}

impl From<enfync::builtin::native::TokioHandle> for ClientConnectorTokio {
    fn from(handle: enfync::builtin::native::TokioHandle) -> Self {
        Self { handle }
    }
}

#[async_trait::async_trait]
impl ClientConnector for ClientConnectorTokio {
    type Handle = enfync::builtin::native::TokioHandle;
    type Message = tungstenite::Message;
    type WSError = tungstenite::error::Error;
    type Socket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    /// Get the connector's runtime handle.
    fn handle(&self) -> Self::Handle {
        self.handle.clone()
    }

    /// Connect to a websocket server.
    ///
    /// Returns `Err` if the request is invalid.
    async fn connect(&self, config: &ClientConfig) -> Result<Self::Socket, Self::WSError> {
        let request = config.connect_http_request();
        let (socket, _) = tokio_tungstenite::connect_async(request).await?;
        Ok(socket)
    }
}
