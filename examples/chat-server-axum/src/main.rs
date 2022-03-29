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

type SessionID = u8;
type Session = ezsockets::Session<SessionID, ()>;

#[derive(Debug)]
enum ChatMessage {
    Broadcast {
        text: String,
        exceptions: Vec<SessionID>,
    },
}

struct ChatServer {
    sessions: HashMap<SessionID, Session>,
    handle: Server<Self>,
}

#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Session = ChatSession;
    type Params = ChatMessage;

    async fn accept(
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

    async fn disconnected(
        &mut self,
        id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), Error> {
        assert!(self.sessions.remove(&id).is_some());
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), Error> {
        match params {
            ChatMessage::Broadcast { exceptions, text } => {
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
    type Params = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }
    async fn text(&mut self, text: String) -> Result<(), Error> {
        tracing::info!("received: {text}");
        self.server
            .call(ChatMessage::Broadcast {
                exceptions: vec![self.id],
                text,
            })
            .await;
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), Error> {
        unimplemented!()
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), Error> {
        match params {
            () => {}
        };
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|handle| ChatServer {
        sessions: HashMap::new(),
        handle,
    });

    let app = Router::new()
        .route("/websocket", get(websocket_handler))
        .layer(Extension(server.clone()));

    let address = SocketAddr::from(([127, 0, 0, 1], 8080));

    tokio::spawn(async move {
        tracing::debug!("listening on {}", address);
        axum::Server::bind(&address)
            .serve(app.into_make_service_with_connect_info::<SocketAddr, _>())
            .await
            .unwrap();
    });

    let stdin = std::io::stdin();
    let lines = stdin.lock().lines();
    for line in lines {
        let line = line.unwrap();
        server
            .call(ChatMessage::Broadcast {
                text: line,
                exceptions: vec![],
            })
            .await;
    }
}

async fn websocket_handler(
    Extension(server): Extension<Server<ChatServer>>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    ezsocket.on_upgrade(server, ())
}
