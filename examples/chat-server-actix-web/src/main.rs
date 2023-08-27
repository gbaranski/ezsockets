use actix_web::App;
use actix_web::HttpRequest;
use actix_web::HttpResponse;
use actix_web::HttpServer;
use actix_web::web;
use async_trait::async_trait;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use std::collections::HashMap;
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

struct AppState {
    server: Server<ChatServer>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|handle| ChatServer {
        sessions: HashMap::new(),
        handle,
    });
    HttpServer::new(move || {
        App::new()
            .route("/ws", web::get().to(index))
            .app_data(web::Data::new(AppState { server: server.clone() }))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}



async fn index(req: HttpRequest, stream: web::Payload, data: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    let (resp, id) = ezsockets::actix_web::accept(req, stream, &data.server, ()).await?;
    tracing::info!(%id, ?resp, "new connection");
    Ok(resp)
}