use async_trait::async_trait;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use ezsockets::axum::Upgrade;
use ezsockets::BoxError;
use ezsockets::Server;
use ezsockets::Session;
use ezsockets::SessionExt;
use ezsockets::Socket;
use std::collections::HashMap;
use std::io::BufRead;
use std::net::SocketAddr;
type SessionID = u8;

#[derive(Debug)]
enum ChatMessage {
    Broadcast {
        text: String,
        exceptions: Vec<SessionID>,
    },
}

struct ChatServer {
    sessions: HashMap<SessionID, Session>,
    handle: Server<ChatMessage>,
}

#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Session = ChatSession;
    type Params = ChatMessage;
    type Args = ();

    async fn accept(&mut self, socket: Socket, _address: SocketAddr, _args: Self::Args) -> Result<Session, BoxError> {
        let id = (0..).find(|i| !self.sessions.contains_key(i)).unwrap_or(0);
        let handle = Session::create(
            |_| ChatSession {
                id,
                server: self.handle.clone(),
            },
            socket,
        );
        self.sessions.insert(id, handle.clone());
        Ok(handle)
    }

    async fn disconnected(
        &mut self,
        id: <Self::Session as SessionExt>::ID,
    ) -> Result<(), BoxError> {
        assert!(self.sessions.remove(&id).is_some());
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), BoxError> {
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
    server: Server<ChatMessage>,
}

#[async_trait]
impl SessionExt for ChatSession {
    type ID = SessionID;
    type Params = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }
    async fn text(&mut self, text: String) -> Result<(), BoxError> {
        self.server
            .call(ChatMessage::Broadcast {
                exceptions: vec![self.id],
                text,
            })
            .await;
        Ok(())
    }

    async fn binary(&mut self, _bytes: Vec<u8>) -> Result<(), BoxError> {
        unimplemented!()
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), BoxError> {
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
    Extension(server): Extension<Server<ChatMessage>>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    ezsocket.on_upgrade(|socket, address| async move {
        server.accept(socket, address, ()).await;
    })
}
