use async_trait::async_trait;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use std::collections::HashMap;
use std::net::SocketAddr;

const DEFAULT_ROOM: &str = "main";

type SessionID = u8;
type Session = ezsockets::Session<SessionID, ()>;

#[derive(Debug)]
enum Message {
    Join {
        id: SessionID,
        room: String,
    },
    Send {
        from: SessionID,
        room: String,
        text: String,
    },
}

struct ChatServer {
    sessions: HashMap<SessionID, Session>,
    rooms: HashMap<String, Vec<SessionID>>,
    handle: Server<Self>,
}

#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Params = Message;
    type Session = SessionActor;

    async fn accept(
        &mut self,
        socket: Socket,
        _address: SocketAddr,
        _args: <Self::Session as ezsockets::SessionExt>::Args,
    ) -> Result<Session, Error> {
        let id = (0..).find(|i| !self.sessions.contains_key(i)).unwrap_or(0);
        let session = Session::create(
            |_handle| SessionActor {
                id,
                server: self.handle.clone(),
                room: DEFAULT_ROOM.to_string(),
            },
            id,
            socket,
        );
        self.sessions.insert(id, session.clone());
        self.rooms.get_mut(DEFAULT_ROOM).unwrap().push(id);
        Ok(session)
    }

    async fn disconnected(
        &mut self,
        id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), Error> {
        assert!(self.sessions.remove(&id).is_some());

        let (ids, n) = self
            .rooms
            .values_mut()
            .find_map(|ids| ids.iter().position(|v| id == *v).map(|n| (ids, n)))
            .expect("could not find session in any room");
        ids.remove(n);
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), Error> {
        match params {
            Message::Send { from, room, text } => {
                let (ids, sessions): (Vec<SessionID>, Vec<&Session>) = self
                    .rooms
                    .get(&room)
                    .unwrap()
                    .iter()
                    .filter(|id| **id != from)
                    .map(|id| (id, self.sessions.get(id).unwrap()))
                    .unzip();

                tracing::info!(
                    "sending {text} to [{sessions}] at `{room}`",
                    sessions = ids
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                for session in sessions {
                    session.text(text.clone());
                }
            }
            Message::Join { id, room } => {
                let (ids, n) = self
                    .rooms
                    .values_mut()
                    .find_map(|ids| ids.iter().position(|v| id == *v).map(|n| (ids, n)))
                    .expect("could not find session in any room");
                ids.remove(n);
                if let Some(ids) = self.rooms.get_mut(&room) {
                    ids.push(id);
                } else {
                    self.rooms.insert(room.clone(), vec![id]);
                }

                let sessions = self
                    .rooms
                    .get(&room)
                    .unwrap()
                    .iter()
                    .map(|id| self.sessions.get(id).unwrap());

                for session in sessions {
                    session.text(format!("User with ID: {id} just joined {room} room"));
                }
            }
        };
        Ok(())
    }
}

struct SessionActor {
    id: SessionID,
    server: Server<ChatServer>,
    room: String,
}

#[async_trait]
impl ezsockets::SessionExt for SessionActor {
    type ID = SessionID;
    type Args = ();
    type Params = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn text(&mut self, text: String) -> Result<(), Error> {
        tracing::info!("received: {text}");
        if text.starts_with('/') {
            let mut args = text.split_whitespace();
            let command = args.next().unwrap();
            if command == "/join" {
                let room = args.next().expect("missing <room> argument").to_string();
                tracing::info!("moving {} to {room}", self.id);
                self.room = room.clone();
                self.server.call(Message::Join { id: self.id, room });
            } else {
                tracing::error!("unrecognized command: {text}");
            }
        } else {
            self.server.call(Message::Send {
                text,
                from: self.id,
                room: self.room.clone(),
            });
        }
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), Error> {
        unimplemented!()
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), Error> {
        let () = params;
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|handle| ChatServer {
        sessions: HashMap::new(),
        rooms: HashMap::from_iter([(DEFAULT_ROOM.to_string(), vec![])]),
        handle,
    });
    ezsockets::tungstenite::run(server, "127.0.0.1:8080", |_| async move { Ok(()) })
        .await
        .unwrap();
}
