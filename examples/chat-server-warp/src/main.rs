use async_trait::async_trait;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use ezsockets::axum::Upgrade;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use std::collections::HashMap;
use std::io::BufRead;
use std::net::SocketAddr;

type SessionID = u16;
type Session = ezsockets::Session<SessionID, ()>;

#[derive(Debug)]
enum ChatMessage {
    Send { from: SessionID, text: String },
}

struct ChatServer {
    sessions: HashMap<SessionID, Session>,
    handle: Server<Self>,
}

#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Session = ChatSession;
    type Call = ChatMessage;

    async fn on_connect(
        &mut self,
        socket: Socket,
        _address: SocketAddr,
        _args: <Self::Session as ezsockets::SessionExt>::Args,
    ) -> Result<Session, Error> {
        let id = (0..).find(|i| !self.sessions.contains_key(i)).unwrap_or(0);
        let session = Session::create(
            |_| ChatSession {
                id,
                server: self.handle.clone(),
            },
            id,
            socket,
        );
        self.sessions.insert(id, session.clone());
        Ok(session)
    }

    async fn on_disconnect(
        &mut self,
        id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), Error> {
        assert!(self.sessions.remove(&id).is_some());
        Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        match call {
            ChatMessage::Send { text, from } => {
                let sessions = self.sessions.iter().filter(|(id, _)| from != **id);
                let text = format!("from {from}: {text}");
                for (id, handle) in sessions {
                    tracing::info!("sending {text} to {id}");
                    handle.text(text.clone());
                }
            }
        };
        Ok(())
    }
}

struct ChatSession {
    id: SessionID,
    server: Server<ChatServer>,
}

#[async_trait]
impl ezsockets::SessionExt for ChatSession {
    type ID = SessionID;
    type Args = ();
    type Call = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }
    async fn on_text(&mut self, text: String) -> Result<(), Error> {
        tracing::info!("received: {text}");
        self.server.call(ChatMessage::Send {
            from: self.id,
            text,
        });
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

async fn websocket_handler(
    Extension(server): Extension<Server<ChatServer>>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    ezsocket.on_upgrade(server, ())
}


#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|handle| ChatServer {
        sessions: HashMap::new(),
        handle,
    });

    let routes = warp::path("echo")
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| {
            // And then our closure will be called when it completes...
            ws.on_upgrade(|websocket| {
                // Just echo all messages back...
                let (tx, rx) = websocket.split();
                rx.forward(tx).map(|result| {
                    if let Err(e) = result {
                        eprintln!("websocket error: {:?}", e);
                    }
                })
            })
        });


    tokio::spawn(async move {
        tracing::debug!("listening on {}", address);
        warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
    });

    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        server.call(ChatMessage::Send {
            text: line,
            from: SessionID::MAX, // reserve some ID for the server
        });
    }


}