use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::Server;
use ezsockets::Session;
use ezsockets::Socket;
use std::net::SocketAddr;

type SessionID = u16;

struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Message = ();
    type Session = SessionActor;

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
    ) -> Result<Session, BoxError> {
        let handle = Session::create(
            |handle| SessionActor {
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

struct SessionActor {
    handle: Session,
    id: SessionID,
}

#[async_trait]
impl ezsockets::SessionExt for SessionActor {
    type ID = SessionID;

    fn id(&self) -> &Self::ID {
        &self.id
    }
    async fn text(&mut self, text: String) -> Result<(), BoxError> {
        self.handle.text(text).await;
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), BoxError> {
        unimplemented!()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|_server| EchoServer {}).await;
    ezsockets::tungstenite::run(server, "127.0.0.1:8080")
        .await
        .unwrap();
}
