use std::net::SocketAddr;

use async_trait::async_trait;
use tokio::net::TcpListener;

type SessionID = u16;
type Session = ezsockets::Session<SessionID, ()>;

struct EchoServer {}

#[async_trait]
impl ezsockets::ServerExt for EchoServer {
    type Session = EchoSession;
    type Call = ();

    async fn on_connect(
        &mut self,
        socket: ezsockets::Socket,
        address: SocketAddr,
        _args: (),
    ) -> Result<Session, ezsockets::Error> {
        let id = address.port();
        let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
        Ok(session)
    }

    async fn on_disconnect(
        &mut self,
        _id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), ezsockets::Error> {
        Ok(())
    }

    async fn on_call(&mut self, params: Self::Call) -> Result<(), ezsockets::Error> {
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
    type Call = ();

    fn id(&self) -> &Self::ID {
        &self.id
    }

    async fn on_text(&mut self, text: String) -> Result<(), ezsockets::Error> {
        self.handle.text(text);
        Ok(())
    }

    async fn on_binary(&mut self, _bytes: Vec<u8>) -> Result<(), ezsockets::Error> {
        unimplemented!()
    }

    async fn on_call(&mut self, params: Self::Call) -> Result<(), ezsockets::Error> {
        let () = params;
        Ok(())
    }
}

pub async fn run(listener: std::net::TcpListener) {
    let listener = TcpListener::from_std(listener).unwrap();
    let (server, _) = ezsockets::Server::create(|_| EchoServer {});
    ezsockets::tungstenite::run_on(server, listener, |_socket| async move { Ok(()) })
        .await
        .unwrap();
}
