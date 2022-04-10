use async_trait::async_trait;
use ezsockets::ClientConfig;
use std::io::BufRead;
use url::Url;

struct Client {}

#[async_trait]
impl ezsockets::ClientExt for Client {
    type Params = ();

    async fn text(&mut self, text: String) -> Result<(), ezsockets::Error> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), ezsockets::Error> {
        let () = params;
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let url = Url::parse("ws://localhost:8080/websocket").unwrap();
    let config = ClientConfig::new(url);
    let (handle, future) = ezsockets::connect(|_client| Client {}, config).await;
    tokio::spawn(async move {
        future.await.unwrap();
    });
    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        tracing::info!("sending {line}");
        handle.text(line).await;
    }
}
