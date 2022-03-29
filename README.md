# ezsockets

Have you ever struggle with creating a WebSocket server or a client in Rust? This crate is for you.

- High level abstraction of WebSocket, handling Ping/Pong from both Client and Server.
- Use of traits to allow declarative and event-based programming.

## Client

#### NOTE: Enable `client` feature to use it.

The code below represents simple client that redirects stdin to the WebSocket server.

```rust
use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::ClientConfig;
use std::io::BufRead;
use url::Url;

struct Client {}

#[async_trait]
impl ezsockets::ClientExt for Client {
    type Params = ();

    async fn text(&mut self, text: String) -> Result<(), BoxError> {
        tracing::info!("received message: {text}");
        Ok(())
    }

    async fn binary(&mut self, bytes: Vec<u8>) -> Result<(), BoxError> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), BoxError> {
        match params {
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
```


## Server

#### NOTE: Enable `server-<backend>` feature to use it.

To create a simple echo server, you'll need to define a `Session` struct.
The code below represents a simple echo server.

```rust
use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::Session;

type SessionID = u16;

struct EchoSession {
    handle: Session,
    id: SessionID,
}

#[async_trait]
impl ezsockets::SessionExt for EchoSession {
    type ID = SessionID;
    type Params = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn text(&mut self, text: String) -> Result<(), BoxError> {
        self.handle.text(text).await; // Send response to the client
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), BoxError> {
        unimplemented!()
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), BoxError> {
        match params {
            () => {}
        }
        Ok(())
    }
}
```

Then, we need to define a `Server` struct


```rust
use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::Server;
use ezsockets::Session;
use ezsockets::Socket;
use std::net::SocketAddr;

struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Params = ();
    type Session = EchoSession;

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
    ) -> Result<Session, BoxError> {
        let handle = Session::create(
            |handle| EchoSession {
                // use port as the SessionID, since we don't have any other meaningful information about the client
                id: address.port(),
                handle,
            },
            socket,
        );
        Ok(handle)
    }

    async fn disconnected(
        &mut self,
        _id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), BoxError> {
        match params {
            () => {}
        };
        Ok(())
    }
}
```

That's all! Now we can start the server. Take a look at the available [Server back-ends](#server-back-ends). For a simple usage, I'd recommend [tokio-tungstenite](#tokio-tungstenite).

## Server back-ends

- [x] [`tokio-tungstenite`](#tokio-tungstenite), a neat Tokio based WebSocket implementation. However, it does not provide fancy features like routing or authentication.
- [x] [`axum`](#axum), an ergonomic and modular web framework built with Tokio, Tower, and Hyper.
- [ ] [`actix-web`](#actix-web) a powerful, pragmatic, and extremely fast web framework for Rust.

### [`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite)

```rust
struct MyServer {}

#[async_trait]
impl ezsockets::ServerExt for MyServer {
    // ...
}

#[tokio::main]
async fn main() {
    let server = ezsockets::Server::create(|_| MyServer {}).await;
    ezsockets::tungstenite::run(server, "127.0.0.1:8080")
        .await
        .unwrap();
}
```

### [`axum`](https://github.com/tokio-rs/axum)

```rust
struct MyServer {}

#[async_trait]
impl ezsockets::ServerExt for MyServer {
    // ...
}

#[tokio::main]
async fn main() {
    let server = ezsockets::Server::create(|_| MyServer {}).await;
    let app = axum::Router::new()
        .route("/websocket", get(websocket_handler))
        .layer(Extension(server.clone()));

    let address = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));

    tokio::spawn(async move {
        tracing::debug!("listening on {}", address);
        axum::Server::bind(&address)
            .serve(app.into_make_service_with_connect_info::<SocketAddr, _>())
            .await
            .unwrap();
    });

}

async fn websocket_handler(
    Extension(server): Extension<ezsockets::Server>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    ezsocket.on_upgrade(|socket, address| async move {
        server.accept(socket, address).await;
    })
}
```

### [`actix-web`](https://github.com/actix/actix-web)

Work in progress!
