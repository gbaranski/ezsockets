use async_trait::async_trait;
use ezsockets::BoxError;
use ezsockets::ServerHandle;
use ezsockets::SessionHandle;
use std::collections::HashMap;
use std::io::BufRead;

type SessionID = u8;

#[derive(Debug)]
enum Message {
    Broadcast {
        text: String,
        exceptions: Vec<SessionID>,
    },
}

struct Server {
    next_id: u8,
    sessions: HashMap<u8, SessionHandle>,
    handle: ServerHandle<Message>,
}

#[async_trait]
impl ezsockets::Server for Server {
    type Message = Message;
    type Session = Session;

    async fn connected(
        &mut self,
        handle: SessionHandle,
        address: std::net::SocketAddr,
    ) -> Result<Self::Session, BoxError> {
        let id = self.next_id;
        self.next_id += 1;
        tracing::info!("connected from {} with id {}", address, id);
        self.sessions.insert(id, handle);
        Ok(Session {
            id,
            server: self.handle.clone(),
        })
    }

    async fn message(&mut self, message: Self::Message) {
        match message {
            Message::Broadcast { exceptions, text } => {
                let sessions = self
                    .sessions
                    .iter()
                    .filter(|(id, _)| !exceptions.contains(id));
                for (id, handle) in sessions {
                    println!("broadcasting {text} to {id}");
                    handle.text(text.clone()).await;
                }
            }
        };
    }
}

struct Session {
    id: SessionID,
    server: ServerHandle<Message>,
}

#[async_trait]
impl ezsockets::Session for Session {
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

    async fn closed(
        &mut self,
        code: Option<ezsockets::CloseCode>,
        reason: Option<String>,
    ) -> Result<(), BoxError> {
        println!("connection closed. close code: {code:#?}. reason: {reason:#?}");
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (handle, _) = ezsockets::run(
        |handle| Server {
            sessions: HashMap::new(),
            handle,
            next_id: 0,
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
