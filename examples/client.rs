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

    async fn closed(
        &mut self,
        code: Option<ezsockets::CloseCode>,
        reason: Option<String>,
    ) -> Result<(), BoxError> {
        println!("connection closed. close code: {code:#?}. reason: {reason:#?}");
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let client = Client {};
    let url = Url::parse("ws://localhost:8080").unwrap();
    let (handle, future) = ezsockets::connect(client, ClientConfig::new(url)).await;
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
