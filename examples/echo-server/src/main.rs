use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::Server;
use ezsockets::SessionHandle;
use ezsockets::Socket;
use std::collections::HashMap;
use std::net::SocketAddr;

type SessionID = u8;

struct EchoServer {
    sessions: HashMap<SessionID, SessionHandle>,
}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Message = ();
    type Session = Session;

    async fn accept(
        &mut self,
        socket: Socket,
        _address: SocketAddr,
    ) -> Result<SessionHandle, BoxError> {
        let id = (0..).find(|i| !self.sessions.contains_key(i)).unwrap_or(0);
        let session = Session {
            id,
        };
        let handle = SessionHandle::create(session, socket);
        self.sessions.insert(id, handle.clone());
        Ok(handle)
    }

    async fn disconnected(
        &mut self,
        id: <Self::Session as ezsockets::Session>::ID,
    ) -> Result<(), BoxError> {
        assert!(self.sessions.remove(&id).is_some());
        Ok(())
    }

    async fn message(&mut self, message: Self::Message) {
        match message {
            () => {}
        };
    }
}

struct Session {
    id: SessionID,
}

#[async_trait]
impl ezsockets::Session for Session {
    type ID = SessionID;

    fn id(&self) -> &Self::ID {
        &self.id
    }
    async fn text(&mut self, text: String) -> Result<Option<ezsockets::Message>, BoxError> {
        Ok(Some(ezsockets::Message::Text(text)))
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<Option<ezsockets::Message>, BoxError> {
        unimplemented!()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|_server| EchoServer {
        sessions: HashMap::new(),
    })
    .await;
    ezsockets::tungstenite::run(server, "127.0.0.1:8080")
        .await
        .unwrap();
}
