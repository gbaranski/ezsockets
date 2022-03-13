use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::ServerHandle;
use ezsockets::SessionHandle;
use ezsockets::Socket;
use std::collections::HashMap;
use std::io::BufRead;
use std::net::SocketAddr;

type SessionID = u8;

#[derive(Debug)]
enum Message {
    Broadcast {
        text: String,
        exceptions: Vec<SessionID>,
    },
}

struct Server {
    sessions: HashMap<u8, SessionHandle>,
    handle: ServerHandle<Server>,
}

#[async_trait]
impl ezsockets::Server for Server {
    type Message = Message;
    type Session = Session;

    async fn accept(
        &mut self,
        socket: Socket,
        _address: SocketAddr,
    ) -> Result<SessionHandle, BoxError> {
        let id = (0..).find(|i| !self.sessions.contains_key(i)).unwrap_or(0);
        let session = Session {
            id,
            server: self.handle.clone(),
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
            Message::Broadcast { exceptions, text } => {
                let sessions = self
                    .sessions
                    .iter()
                    .filter(|(id, _)| !exceptions.contains(id));
                for (id, handle) in sessions {
                    tracing::info!("broadcasting {text} to {id}");
                    handle.text(text.clone()).await;
                }
            }
        };
    }
}

struct Session {
    id: SessionID,
    server: ServerHandle<Server>,
}

#[async_trait]
impl ezsockets::Session for Session {
    type ID = SessionID;

    fn id(&self) -> &Self::ID {
        &self.id
    }
    async fn text(&mut self, text: String) -> Result<Option<ezsockets::Message>, BoxError> {
        self.server
            .call(Message::Broadcast {
                exceptions: vec![self.id],
                text,
            })
            .await;
        Ok(None)
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<Option<ezsockets::Message>, BoxError> {
        unimplemented!()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (handle, _) = ezsockets::run(
        |handle| Server {
            sessions: HashMap::new(),
            handle,
        },
        "127.0.0.1:8080",
    )
    .await;
    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        handle
            .call(Message::Broadcast {
                text: line,
                exceptions: vec![],
            })
            .await;
    }
}
