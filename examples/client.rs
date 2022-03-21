use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::ClientConfig;
use std::io::BufRead;
use url::Url;

struct Client {
    #[allow(dead_code)]
    client: ezsockets::Client<<Self as ezsockets::ClientExt>::Message>,
}

#[async_trait]
impl ezsockets::ClientExt for Client {
    type Message = ();

    async fn text(&mut self, text: String) -> Result<(), BoxError> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), BoxError> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn closed(&mut self) -> Result<(), BoxError> {
        Ok(())
    }

    async fn call(&mut self, message: Self::Message) {
        match message {
            () => {}
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let url = Url::parse("ws://localhost:3000/websocket").unwrap();
    let config = ClientConfig::new(url).basic("username", "password");
    let (handle, future) = ezsockets::connect(|client| Client { client }, config).await;
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
