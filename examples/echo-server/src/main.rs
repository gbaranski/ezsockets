use async_trait::async_trait;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use std::net::SocketAddr;

type SessionID = u16;
type Session = ezsockets::Session<SessionID, ()>;

struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Session = EchoSession;
    type Params = ();

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        _args: (),
    ) -> Result<Session, Error> {
        let id = address.port();
        let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
        Ok(session)
    }

    async fn disconnected(
        &mut self,
        _id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), Error> {
        Ok(())
    }

    async fn call(&mut self, params: Self::Params) -> Result<(), Error> {
        let () = params;
        Ok(())
    }
}

struct EchoSession {
    handle: Session,
    id: SessionID,
}

#[async_trait]
impl ezsockets::SessionExt for EchoSession {
    type ID = SessionID;
    type Args = ();
    type Params = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn text(&mut self, text: String) -> Result<(), Error> {
        self.handle.text(text).await;
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
    let (server, _) = Server::create(|_server| EchoServer {});
    ezsockets::tungstenite::run(server, "127.0.0.1:8080", |_| async move { Ok(()) })
        .await
        .unwrap();
}
