use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::ClientConfig;
use std::io::BufRead;
use url::Url;

struct Client {}

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

    async fn message(&mut self, message: Self::Message) -> Result<(), BoxError> {
        match message {
            () => {}
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let url = format!("ws://127.0.0.1:8080");
    let url = Url::parse(&url).unwrap();
    let config = ClientConfig::new(url);
    let (handle, future) = ezsockets::connect(|_| Client {}, config).await;
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
