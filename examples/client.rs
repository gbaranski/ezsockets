use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::ClientConfig;
use std::io::BufRead;
use url::Url;

struct Client {}

#[async_trait]
impl ezsockets::Client for Client {
    async fn text(&mut self, text: String) -> Result<Option<ezsockets::Message>, BoxError> {
        println!("received message: {text}");
        Ok(None)
    }

    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<ezsockets::Message>, BoxError> {
        println!("received bytes: {bytes:?}");
        Ok(None)
    }

    async fn closed(&mut self) -> Result<(), BoxError> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let client = Client {};
    let url = Url::parse("ws://localhost:3000/websocket").unwrap();
    let config = ClientConfig::new(url).basic("username", "password");
    let (handle, future) = ezsockets::connect(client, config).await;
    tokio::spawn(async move {
        future.await.unwrap();
    });
    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        handle.text(line).await;
    }
}
