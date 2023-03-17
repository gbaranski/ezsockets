use async_trait::async_trait;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use native_tls::Identity;
use std::net::SocketAddr;
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
        socket: Socket,
        address: SocketAddr,
        _args: (),
    ) -> Result<Session, Error> {
        let id = address.port();
        let session = Session::create(|handle| EchoSession { id, handle }, id, socket);
        Ok(session)
    }

    async fn on_disconnect(
        &mut self,
        _id: <Self::Session as ezsockets::SessionExt>::ID,
    ) -> Result<(), Error> {
        Ok(())
    }

    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        let () = call;
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

    async fn on_text(&mut self, text: String) -> Result<(), Error> {
        self.handle.text(text);
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let der = include_bytes!("../identity.p12");
    let cert = Identity::from_pkcs12(der, "mypass").unwrap();
    let tls_acceptor = tokio_native_tls::TlsAcceptor::from(
        native_tls::TlsAcceptor::builder(cert).build().unwrap(),
    );
    let tls_acceptor = ezsockets::tungstenite::Acceptor::NativeTls(tls_acceptor);

    let (server, _) = Server::create(|_server| EchoServer {});
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    ezsockets::tungstenite::run_on(server, listener, tls_acceptor, |_| async move { Ok(()) })
        .await
        .unwrap();
}
