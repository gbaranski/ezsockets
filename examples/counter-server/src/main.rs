use async_trait::async_trait;
use ezsockets::Error;
use ezsockets::Server;
use ezsockets::Socket;
use std::net::SocketAddr;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(1);

type SessionID = u16;
type Session = ezsockets::Session<SessionID, Message>;

struct CounterServer {}

#[async_trait]
impl ezsockets::ServerExt for CounterServer {
    type Session = CounterSession;
    type Params = ();

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        _args: (),
    ) -> Result<Session, Error> {
        let id = address.port();
        let session = Session::create(
            |handle| {
                let counting_task = tokio::spawn({
                    let session = handle.clone();
                    async move {
                        loop {
                            session.call(Message::Increment).await;
                            session.call(Message::Share).await;
                            tokio::time::sleep(INTERVAL).await;
                        }
                    }
                });
                CounterSession {
                    id,
                    handle,
                    counter: 0,
                    counting_task,
                }
            },
            id,
            socket,
        );
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

struct CounterSession {
    handle: Session,
    id: SessionID,
    counter: usize,
    counting_task: tokio::task::JoinHandle<()>,
}

impl Drop for CounterSession {
    fn drop(&mut self) {
        self.counting_task.abort();
    }
}
#[derive(Debug)]
enum Message {
    // increment current counter by 1
    Increment,
    // share current counter with server
    Share,
}

#[async_trait]
impl ezsockets::SessionExt for CounterSession {
    type ID = SessionID;
    type Args = ();
    type Params = Message;

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
        match params {
            Message::Increment => self.counter += 1,
            Message::Share => self.handle.text(format!("counter: {}", self.counter)).await,
        };
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|_server| CounterServer {});
    ezsockets::tungstenite::run(server, "127.0.0.1:8080", |_| async move { Ok(()) })
        .await
        .unwrap();
}
