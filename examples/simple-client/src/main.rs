use async_trait::async_trait;
use ezsockets::ClientConfig;
use ezsockets::CloseCode;
use ezsockets::CloseFrame;
use ezsockets::Error;
use std::io::BufRead;
use url::Url;

enum Call {
    NewLine(String),
}

struct Client {
    handle: ezsockets::Client<Self>,
}

#[async_trait]
impl ezsockets::ClientExt for Client {
    type Call = Call;

    async fn on_text(&mut self, text: String) -> Result<(), Error> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        match call {
            Call::NewLine(line) => {
                if line == "exit" {
                    tracing::info!("exiting...");
                    self.handle
                        .close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "adios!".to_string(),
                        }))
                        .unwrap();
                    return Ok(());
                }
                tracing::info!("sending {line}");
                self.handle.text(line).unwrap();
            }
        };
        Ok(())
    }

    async fn on_ping(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        tracing::info!("received ping: {bytes:?}");
        tracing::info!("sending pong");
        self.handle.pong(bytes)?;
        Ok(())
    }

    async fn on_pong(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        tracing::info!("received pong: {bytes:?}");
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
    let (handle, future) = ezsockets::connect(|handle| Client { handle }, config).await;
    tokio::spawn(async move {
        let stdin = std::io::stdin();
        let lines = stdin.lock().lines();
        for line in lines {
            let line = line.unwrap();
            handle.call(Call::NewLine(line)).unwrap();
        }
    });
    future.await.unwrap();
}
