use async_trait::async_trait;
use ezsockets::CloseFrame;
use ezsockets::Error;
use ezsockets::Server;
use std::net::SocketAddr;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(1);

type SessionID = u16;
type Session = ezsockets::Session<SessionID, Message>;

struct CounterServer {}

#[async_trait]
impl ezsockets::ServerExt for CounterServer {
    type Session = CounterSession;
    type Call = ();

    async fn on_connect(
        &mut self,
        socket: ezsockets::Socket,
        _request: ezsockets::Request,
        address: SocketAddr,
    ) -> Result<Session, Option<CloseFrame>> {
        let id = address.port();
        let session = Session::create(
            |handle| {
                let counting_task = tokio::spawn({
                    let session = handle.clone();
                    async move {
                        loop {
                            session.call(Message::Increment);
                            session.call(Message::Share);
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
    type Call = Message;

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
        match call {
            Message::Increment => self.counter += 1,
            Message::Share => self.handle.text(format!("counter: {}", self.counter)),
        };
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let (server, _) = Server::create(|_server| CounterServer {});
    ezsockets::tungstenite::run(server, "127.0.0.1:8080")
        .await
        .unwrap();
}
