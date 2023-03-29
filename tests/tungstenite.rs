mod chat;
mod client;

use chat::ChatClient;
use chat::ChatServer;

use ezsockets::Server;
use ezsockets::ServerExt;
use std::net::SocketAddr;
use tokio::net::TcpListener;

async fn run<E>(create_fn: impl FnOnce(Server<E>) -> E) -> (Server<E>, SocketAddr)
where
    E: ServerExt + 'static,
{
    let (server, _) = Server::create(create_fn);
    let address = SocketAddr::from(([127, 0, 0, 1], 0));

    tracing::debug!("listening on {}", address);
    let listener = TcpListener::bind(address).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn({
        let server = server.clone();
        async move {
            ezsockets::tungstenite::run_on(
                server,
                listener,
                ezsockets::tungstenite::Acceptor::Plain
            )
            .await
            .unwrap();
        }
    });
    (server, address)
}

#[tokio::test]
async fn test_tungstenite_chat() {
    tracing_subscriber::fmt::init();
    let (_, address) = run(ChatServer::new).await;
    let alice = client::connect(ChatClient::new, address).await;
    let bob = client::connect(ChatClient::new, address).await;
    chat::test(alice, bob).await;
}
