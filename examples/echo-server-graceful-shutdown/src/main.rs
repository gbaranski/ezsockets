// An echo server that demonstrates a clean graceful shutdown:
//
// - On Ctrl-C, the server stops accepting new connections, sends a Close frame
//   to every live session, waits for each session to drain through
//   `on_disconnect`, and only then exits.
//
// Try it: `cargo run`, connect with `websocat ws://127.0.0.1:8080`,
//         exchange a few messages, then press Ctrl-C in the server.

use async_trait::async_trait;
use ezsockets::CloseFrame;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use std::net::SocketAddr;

type SessionID = u16;
type Session = ezsockets::Session<SessionID, ()>;

struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Session = EchoSession;
    type Call = ();

    async fn on_connect(
        &mut self,
        socket: Socket,
        _request: ezsockets::Request,
        address: SocketAddr,
    ) -> Result<Session, Option<CloseFrame>> {
        let id = address.port();
        let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
        Ok(session)
    }

    async fn on_disconnect(
        &mut self,
        id: SessionID,
        reason: Result<Option<CloseFrame>, Error>,
    ) -> Result<(), Error> {
        tracing::info!(%id, ?reason, "session disconnected");
        Ok(())
    }

    async fn on_call(&mut self, _call: ()) -> Result<(), Error> {
        Ok(())
    }
}

struct EchoSession {
    id: SessionID,
    handle: Session,
}

#[async_trait]
impl ezsockets::SessionExt for EchoSession {
    type ID = SessionID;
    type Call = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn on_text(&mut self, text: ezsockets::Utf8Bytes) -> Result<(), Error> {
        self.handle.text(text)?;
        Ok(())
    }

    async fn on_binary(&mut self, _bytes: ezsockets::Bytes) -> Result<(), Error> {
        Ok(())
    }

    async fn on_call(&mut self, _call: ()) -> Result<(), Error> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, server_task) = Server::create(|_handle| EchoServer {});

    // Run the tungstenite acceptor on a background task. When we want to stop
    // accepting new TCP connections we simply abort this task.
    let address = SocketAddr::from(([127, 0, 0, 1], 8080));
    let acceptor_task = {
        let server = server.clone();
        tokio::spawn(async move {
            tracing::info!("listening on {address}");
            ezsockets::tungstenite::run(server, address).await.unwrap();
        })
    };

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    tracing::info!("ctrl-c received — initiating graceful shutdown");

    // Stop taking new TCP connections, then drain live sessions.
    acceptor_task.abort();
    server
        .graceful_shutdown()
        .await
        .expect("server actor unexpectedly stopped");

    // The server actor finishes once graceful_shutdown resolves.
    server_task.await.expect("server actor task panicked");
    tracing::info!("clean exit");
}
