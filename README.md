# ezsockets

Creating a WebSocket server or a client in Rust can be troublesome. This crate facilitates this process by providing:

- High-level abstraction of WebSocket, handling Ping/Pong from both Client and Server.
- Traits to allow declarative and event-based programming.
- Automatic reconnection of WebSocket Client.

View the full documentation at [docs.rs/ezsockets](http://docs.rs/ezsockets)

## Client

The code below represents a simple client that redirects stdin to the WebSocket server.

```rust
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
    let (handle, future) = ezsockets::connect(|_client| Client { }, config).await;
    tokio::spawn(async move {
        future.await.unwrap();
    });
    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        tracing::info!("sending {line}");
        handle.text(line);
    }
}

```


## Server

To create a simple echo server, we need to define a `Session` struct.
The code below represents a simple echo server.

```rust
use async_trait::async_trait;
use ezsockets::Session;

type SessionID = u16;

struct EchoSession {
    handle: Session,
    id: SessionID,
}

#[async_trait]
impl ezsockets::SessionExt for EchoSession {
    type ID = SessionID;
    type Args = ();
    type Params = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn text(&mut self, text: String) -> Result<(), ezsockets::Error> {
        self.handle.text(text); // Send response to the client
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
        unimplemented!()
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), ezsockets::Error> {
        let () = params;
        Ok(())
    }
}
```

Then, we need to define a `Server` struct


```rust
use async_trait::async_trait;
use ezsockets::Server;
use ezsockets::Session;
use ezsockets::Socket;
use std::net::SocketAddr;

struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Session = EchoSession;
    type Params = ();

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        _args: (),
    ) -> Result<Session, ezsockets::Error> {
        let id = address.port();
        let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
        Ok(session)
    }

    async fn disconnected(
        &mut self,
        _id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), ezsockets::Error> {
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), ezsockets::Error> {
        let () = params;
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

Enable using
```toml
ezsockets = { version = "0.3", features = ["tungstenite"] }
```

```rust
struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    // ...
}

#[tokio::main]
async fn main() {
    let (server, _) = ezsockets::Server::create(|_| EchoServer {});
    ezsockets::tungstenite::run(server, "127.0.0.1:8080", |_socket| async move { Ok(()) })
        .await
        .unwrap();
}
```

### [`axum`](https://github.com/tokio-rs/axum)

Enable using
```toml
ezsockets = { version = "0.3", features = ["axum"] }
```

```rust
struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    // ...
}

#[tokio::main]
async fn main() {
    let (server, _) = ezsockets::Server::create(|_| EchoServer {});
    let app = axum::Router::new()
        .route("/websocket", get(websocket_handler))
        .layer(Extension(server.clone()));

    let address = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));

    tokio::spawn(async move {
        tracing::debug!("listening on {}", address);
        axum::Server::bind(&address)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });

}

async fn websocket_handler(
    Extension(server): Extension<ezsockets::Server>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    ezsocket.on_upgrade(|socket, address| async move {
        server.accept(socket, address, ()).await;
    })
}
```

### [`actix-web`](https://github.com/actix/actix-web)

Work in progress!
