use async_trait::async_trait;
use ezsockets::CloseFrame;
use ezsockets::Error;
use ezsockets::Request;
use ezsockets::Server;
use ezsockets::Socket;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

const DEFAULT_ROOM: &str = "main";

type SessionID = u8;
type Session = ezsockets::Session<SessionID, ()>;

#[derive(Debug)]
pub enum Message {
    Join {
        id: SessionID,
        room: String,
        respond_to: oneshot::Sender<()>,
    },
    Send {
        from: SessionID,
        room: String,
        text: String,
    },
}

pub struct ChatServer {
    sessions: HashMap<SessionID, Session>,
    rooms: HashMap<String, Vec<SessionID>>,
    handle: Server<Self>,
}

impl ChatServer {
    pub fn new(handle: Server<Self>) -> Self {
        Self {
            sessions: Default::default(),
            rooms: HashMap::from_iter([(DEFAULT_ROOM.to_string(), vec![])]),
            handle,
        }
    }
}

#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Call = Message;
    type Session = SessionActor;

    async fn on_connect(
        &mut self,
        socket: Socket,
        request: Request,
        _address: SocketAddr,
    ) -> Result<Session, Option<CloseFrame>> {
        let value = request.headers().get("Some-Header").unwrap();
        assert_eq!(value, "someValue");
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

    async fn on_disconnect(
        &mut self,
        id: <Self::Session as ezsockets::SessionExt>::ID,
        _reason: Result<Option<CloseFrame>, Error>,
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

    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        match call {
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
                    session.text(text.clone()).unwrap();
                }
            }
            Message::Join {
                id,
                room,
                respond_to,
            } => {
                tracing::info!("joining {id} to {room}");
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
                    .filter(|v| **v != id)
                    .map(|id| self.sessions.get(id).unwrap());

                respond_to.send(()).unwrap();
                for session in sessions {
                    session
                        .text(format!("User with ID: {id} just joined {room} room"))
                        .unwrap();
                }
            }
        };
        Ok(())
    }
}

pub struct SessionActor {
    id: SessionID,
    server: Server<ChatServer>,
    room: String,
}

#[async_trait]
impl ezsockets::SessionExt for SessionActor {
    type ID = SessionID;
    type Call = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn on_text(&mut self, text: String) -> Result<(), Error> {
        tracing::info!("received: {text}");
        if text.starts_with('/') {
            let mut args = text.split_whitespace();
            let command = args.next().unwrap();
            if command == "/join" {
                let room = args.next().expect("missing <room> argument").to_string();
                tracing::info!("moving {} to {room}", self.id);
                self.room = room.clone();
                self.server
                    .call_with(|respond_to| Message::Join {
                        id: self.id,
                        room,
                        respond_to,
                    })
                    .await
                    .unwrap();
            } else {
                tracing::error!("unrecognized command: {text}");
            }
        } else {
            self.server
                .call(Message::Send {
                    text,
                    from: self.id,
                    room: self.room.clone(),
                })
                .unwrap();
        }
        Ok(())
    }

    async fn on_binary(&mut self, _bytes: Vec<u8>) -> Result<(), Error> {
        unimplemented!()
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        let () = call;
        Ok(())
    }
}

use ezsockets::Client;
use tokio::sync::broadcast;
use tokio::sync::oneshot;

pub struct ChatClient {
    handle: Client<Self>,
    messages: broadcast::Sender<String>,
}

#[derive(Debug)]
pub enum ChatClientMessage {
    Send(String),
    Subscribe(oneshot::Sender<broadcast::Receiver<String>>),
}

impl ChatClient {
    pub fn new(handle: Client<Self>) -> Self {
        let (sender, _) = broadcast::channel(8);
        Self {
            handle,
            messages: sender,
        }
    }
}

#[async_trait]
impl ezsockets::ClientExt for ChatClient {
    type Call = ChatClientMessage;

    async fn on_text(&mut self, text: String) -> Result<(), ezsockets::Error> {
        tracing::info!("received message: {text}");
        let _ = self.messages.send(text);
        Ok(())
    }

    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
        tracing::info!("received bytes: {bytes:?}");
        Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), ezsockets::Error> {
        match call {
            ChatClientMessage::Send(message) => self.handle.text(message).unwrap(),
            ChatClientMessage::Subscribe(respond_to) => {
                respond_to.send(self.messages.subscribe()).unwrap()
            }
        }
        Ok(())
    }
}

pub async fn test(alice: Client<ChatClient>, bob: Client<ChatClient>) {
    let mut bob_messages = bob.call_with(ChatClientMessage::Subscribe).await.unwrap();
    let mut alice_messages = alice.call_with(ChatClientMessage::Subscribe).await.unwrap();
    alice
        .call(ChatClientMessage::Send("Hi Bob!".to_string()))
        .unwrap();
    alice
        .call(ChatClientMessage::Send("Cya Bob!".to_string()))
        .unwrap();
    assert_eq!(bob_messages.recv().await.unwrap(), "Hi Bob!".to_string());
    assert_eq!(bob_messages.recv().await.unwrap(), "Cya Bob!".to_string());
    alice
        .call(ChatClientMessage::Send(format!("/join abc")))
        .unwrap();

    alice
        .call(ChatClientMessage::Send("Is there anyone?".to_string()))
        .unwrap(); // no

    tokio::time::sleep(Duration::from_millis(100)).await; // sorry for this hack, but i can't find a better solution right now
    bob.call(ChatClientMessage::Send(format!("/join abc")))
        .unwrap();

    assert_eq!(
        alice_messages.recv().await.unwrap(),
        "User with ID: 1 just joined abc room".to_string()
    );
    alice
        .call(ChatClientMessage::Send("Hi in the new room".to_string()))
        .unwrap();
    assert_eq!(
        bob_messages.recv().await.unwrap(),
        "Hi in the new room".to_string()
    );
}
