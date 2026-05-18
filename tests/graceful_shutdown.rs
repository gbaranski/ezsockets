mod client;

use async_trait::async_trait;
use ezsockets::CloseCode;
use ezsockets::CloseFrame;
use ezsockets::Error;
use ezsockets::Request;
use ezsockets::Server;
use ezsockets::Socket;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Notify;

type SessionID = u32;
type Session = ezsockets::Session<SessionID, ()>;

struct TestServer {
    connect_count: Arc<AtomicUsize>,
    disconnect_count: Arc<AtomicUsize>,
    disconnect_close_codes: Arc<std::sync::Mutex<Vec<Option<u16>>>>,
    next_id: u32,
    connected: Arc<Notify>,
}

#[async_trait]
impl ezsockets::ServerExt for TestServer {
    type Session = TestSession;
    type Call = ();

    async fn on_connect(
        &mut self,
        socket: Socket,
        _request: Request,
        _address: SocketAddr,
    ) -> Result<Session, Option<CloseFrame>> {
        let id = self.next_id;
        self.next_id += 1;
        let session = Session::create(|_| TestSession { id }, id, socket);
        self.connect_count.fetch_add(1, Ordering::SeqCst);
        self.connected.notify_waiters();
        Ok(session)
    }

    async fn on_disconnect(
        &mut self,
        _id: SessionID,
        reason: Result<Option<CloseFrame>, Error>,
    ) -> Result<(), Error> {
        let code = match reason {
            Ok(Some(CloseFrame { code, .. })) => Some(u16::from(code)),
            _ => None,
        };
        self.disconnect_close_codes.lock().unwrap().push(code);
        self.disconnect_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_call(&mut self, _call: ()) -> Result<(), Error> {
        Ok(())
    }
}

struct TestSession {
    id: SessionID,
}

#[async_trait]
impl ezsockets::SessionExt for TestSession {
    type ID = SessionID;
    type Call = ();

    fn id(&self) -> &SessionID {
        &self.id
    }

    async fn on_text(&mut self, _text: ezsockets::Utf8Bytes) -> Result<(), Error> {
        Ok(())
    }

    async fn on_binary(&mut self, _bytes: ezsockets::Bytes) -> Result<(), Error> {
        Ok(())
    }

    async fn on_call(&mut self, _call: ()) -> Result<(), Error> {
        Ok(())
    }
}

async fn wait_for<F: Fn() -> bool>(notify: &Notify, predicate: F) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if predicate() {
                return;
            }
            notify.notified().await;
        }
    })
    .await
    .expect("timed out waiting for condition");
}

async fn spawn_server() -> (
    Server<TestServer>,
    SocketAddr,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    Arc<std::sync::Mutex<Vec<Option<u16>>>>,
    Arc<Notify>,
    tokio::task::JoinHandle<()>,
) {
    let connect_count = Arc::new(AtomicUsize::new(0));
    let disconnect_count = Arc::new(AtomicUsize::new(0));
    let disconnect_close_codes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let connected = Arc::new(Notify::new());
    let (server, actor_join) = Server::create({
        let connect_count = connect_count.clone();
        let disconnect_count = disconnect_count.clone();
        let disconnect_close_codes = disconnect_close_codes.clone();
        let connected = connected.clone();
        move |_| TestServer {
            connect_count,
            disconnect_count,
            disconnect_close_codes,
            next_id: 0,
            connected,
        }
    });

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn({
        let server = server.clone();
        async move {
            let _ = ezsockets::tungstenite::run_on(
                server,
                listener,
                ezsockets::tungstenite::Acceptor::Plain,
            )
            .await;
        }
    });

    (
        server,
        address,
        connect_count,
        disconnect_count,
        disconnect_close_codes,
        connected,
        actor_join,
    )
}

#[tokio::test]
async fn graceful_shutdown_drains_active_sessions() {
    let (server, address, connect_count, disconnect_count, close_codes, connected, actor_join) =
        spawn_server().await;

    // Connect three clients and wait until they have all registered server-side.
    let _c1 = client::connect(EchoClient::new, address).await;
    let _c2 = client::connect(EchoClient::new, address).await;
    let _c3 = client::connect(EchoClient::new, address).await;
    wait_for(&connected, || connect_count.load(Ordering::SeqCst) >= 3).await;

    // Trigger graceful shutdown — should resolve only after all sessions drain.
    server.graceful_shutdown().await.unwrap();

    assert_eq!(disconnect_count.load(Ordering::SeqCst), 3);
    let codes = close_codes.lock().unwrap().clone();
    assert_eq!(codes.len(), 3);
    for code in codes {
        assert_eq!(
            code,
            Some(u16::from(CloseCode::Away)),
            "expected sessions to be closed with CloseCode::Away (1001)"
        );
    }

    // Server actor task should now have terminated.
    tokio::time::timeout(Duration::from_secs(2), actor_join)
        .await
        .expect("server actor did not exit")
        .expect("server actor task panicked");
}

#[tokio::test]
async fn graceful_shutdown_with_no_sessions_completes_immediately() {
    let (server, _address, connect_count, disconnect_count, _codes, _connected, actor_join) =
        spawn_server().await;

    assert_eq!(connect_count.load(Ordering::SeqCst), 0);
    tokio::time::timeout(Duration::from_secs(2), server.graceful_shutdown())
        .await
        .expect("graceful_shutdown stalled with zero sessions")
        .unwrap();

    assert_eq!(disconnect_count.load(Ordering::SeqCst), 0);
    tokio::time::timeout(Duration::from_secs(2), actor_join)
        .await
        .expect("server actor did not exit")
        .expect("server actor task panicked");
}

#[tokio::test]
async fn graceful_shutdown_is_idempotent_across_concurrent_callers() {
    let (server, address, connect_count, disconnect_count, _codes, connected, actor_join) =
        spawn_server().await;

    let _c1 = client::connect(EchoClient::new, address).await;
    let _c2 = client::connect(EchoClient::new, address).await;
    wait_for(&connected, || connect_count.load(Ordering::SeqCst) >= 2).await;

    // Two concurrent shutdown callers: both must succeed and both must observe
    // the drain having completed.
    let a = server.clone();
    let b = server.clone();
    let (ra, rb) = tokio::join!(a.graceful_shutdown(), b.graceful_shutdown());
    ra.unwrap();
    rb.unwrap();

    assert_eq!(disconnect_count.load(Ordering::SeqCst), 2);

    // A third call, made after the actor has exited, must return ServerStopped.
    let third = server.graceful_shutdown().await;
    assert!(matches!(
        third,
        Err(ezsockets::GracefulShutdownError::ServerStopped)
    ));

    tokio::time::timeout(Duration::from_secs(2), actor_join)
        .await
        .expect("server actor did not exit")
        .expect("server actor task panicked");
}

// ---- minimal echo client used by the tests above ----

use ezsockets::Client;
use ezsockets::ClientExt;

pub struct EchoClient {
    _handle: Client<Self>,
}

impl EchoClient {
    pub fn new(handle: Client<Self>) -> Self {
        Self { _handle: handle }
    }
}

#[async_trait]
impl ClientExt for EchoClient {
    type Call = ();

    async fn on_text(&mut self, _text: ezsockets::Utf8Bytes) -> Result<(), Error> {
        Ok(())
    }

    async fn on_binary(&mut self, _bytes: ezsockets::Bytes) -> Result<(), Error> {
        Ok(())
    }

    async fn on_call(&mut self, _call: ()) -> Result<(), Error> {
        Ok(())
    }
}
