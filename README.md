# ezsockets

Have you ever had troubles building a WebSocket server or a client in Rust? This crate might come very handy.

- High level abstraction of WebSocket, handling Ping/Pong from both Client and Server
- Use of traits to allow declarative and event-based programming


To create a simple echo server, you'll need to first define a `Session` struct
Simplest echo server can be represented by the following code:

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
}
```

After that, we'll also need a `Server` struct


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
    type Message = ();
    type Session = EchoSession;

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
    ) -> Result<Session, BoxError> {
        let handle = Session::create(
            |handle| EchoSession {
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

    async fn message(&mut self, message: Self::Message) {
        match message {
            () => {}
        };
    }
}
```


And that's it! We got that, now we need to start the server somehow, take a look at available [Server back-ends](#server-back-ends), for simplest usage, I'd recommend [tokio-tungstenite](#tokio-tungstenite)

## Server back-ends

- [x] [`tokio-tungstenite`](#tokio-tungstenite), a Tokio based WebSocket implementation, althought it's not possible to use fancy features, like routing or authentication, because it does not use T
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
    Extension(server): Extension<ezsockets::Server<MyServer>>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    ezsocket.on_upgrade(|socket, address| async move {
        server.accept(socket, address).await;
    })
}
```

### [`actix-web`](https://github.com/actix/actix-web)

Work in progress!