use async_trait::async_trait;
use ezsockets::ClientConfig;
use ezsockets::ClientExt;
use std::io::BufRead;
use url::Url;
use serde::Serialize;
use serde::Deserialize;

#[derive(Debug, Serialize, Deserialize)]
enum Data {
    Increment(usize),
    Decrement(usize),
}

struct Client {
    counter: usize,
}

#[async_trait]
impl ezsockets::JsonClientExt<'static, (), Data> for Client {
    async fn json(&mut self, data: Data) -> Result<(), ezsockets::Error> {
        tracing::info!("received data: {data:?}");
        let before = self.counter;
        self.counter = match data {
            Data::Increment(n) => self.counter + n,
            Data::Decrement(n) => self.counter - n,
        };
        tracing::info!("before={} after={}", before, self.counter);
        Ok(())
    }

    async fn call(&mut self, params: ()) -> Result<(), ezsockets::Error> {
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
        ClientExt::text(&mut handle, line).await;
        // handle.text(line).await;
    }
}
