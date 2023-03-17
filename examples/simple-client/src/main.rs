use async_trait::async_trait;
use ezsockets::ClientConfig;
use ezsockets::CloseCode;
use ezsockets::CloseFrame;
use ezsockets::Error;
use std::io::BufRead;
use url::Url;

struct Client {}

#[async_trait]
impl ezsockets::ClientExt for Client {
    type Params = ();

    async fn text(&mut self, text: String) -> Result<(), Error> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), Error> {
        let () = params;
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let mut args = std::env::args();
    let url = args
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:8080".to_string());
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
        if line == "exit" {
            tracing::info!("exiting...");
            handle
                .close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: "adios!".to_string(),
                }))
                .await;
            return;
        }
        tracing::info!("sending {line}");
        handle.text(line);
    }
}
