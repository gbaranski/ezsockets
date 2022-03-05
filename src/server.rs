use crate::BoxError;
use crate::CloseFrame;
use crate::Session;
use crate::SessionActor;
use crate::SessionHandle;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::Instrument;
use tracing::Span;

struct ServerActor<E: Server> {
    receiver: mpsc::UnboundedReceiver<E::Message>,
    extension: E,
    address: SocketAddr,
}

impl<E: Server> ServerActor<E>
where
    E: Send + 'static,
    E::Message: Send,
    <E::Session as Session>::ID: Send,
{
    async fn run(&mut self) -> Result<(), BoxError> {
        tracing::info!("starting server on {}", self.address);
        let listener = tokio::net::TcpListener::bind(self.address).await?;
        loop {
            tokio::select! {
                Ok((stream, address)) = listener.accept() => {
                    self.accept_connection(stream, address).await?;
                }
                Some(message) = self.receiver.recv() => {
                    self.extension.message(message).await;
                }
                else => break,
            }
        }
        Ok(())
    }

    async fn accept_connection(
        &mut self,
        stream: TcpStream,
        address: SocketAddr,
    ) -> Result<(), BoxError> {
        let websocket_stream = tokio_tungstenite::accept_async(stream).await?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let handle = SessionHandle::new(sender);
        let extension = self.extension.connected(handle, address).await?;
        let id = extension.id().clone();
        tracing::info!(%id, %address, "connected");
        let mut actor =
            SessionActor::new(extension, id, receiver, websocket_stream, Instant::now());
        tokio::spawn(
            async move {
                let result = actor.run().await;
                let id = actor.id;
                match result {
                    Ok(Some(CloseFrame { code, reason })) => {
                        tracing::info!(%id, ?code, %reason, "connection closed")
                    }
                    Ok(None) => tracing::info!(%id, "connection closed"),
                    Err(err) => tracing::warn!(%id, "connection error: {err}"),
                };
                actor.extension.disconnected().await?;
                Ok::<(), BoxError>(())
            }
            .instrument(Span::current()),
        );
        Ok(())
    }
}

#[async_trait]
pub trait Server: Send {
    type Session: Session;
    type Message;

    async fn connected(
        &mut self,
        handle: SessionHandle,
        address: SocketAddr,
    ) -> Result<Self::Session, BoxError>;
    async fn message(&mut self, message: Self::Message);
}

#[derive(Debug)]
pub struct ServerHandle<E: Server> {
    sender: mpsc::UnboundedSender<E::Message>,
}

impl<E: Server> ServerHandle<E>
where
    E::Message: std::fmt::Debug,
{
    pub async fn call(&self, message: E::Message) {
        self.sender.send(message).unwrap();
    }
}

impl<E: Server> std::clone::Clone for ServerHandle<E> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

pub async fn run<E: Server>(
    create: impl FnOnce(ServerHandle<E>) -> E,
    address: impl ToSocketAddrs,
) -> (ServerHandle<E>, impl Future<Output = Result<(), BoxError>>)
where
    E: Server + 'static,
    E::Message: Send + 'static,
    <E::Session as Session>::ID: Send + 'static,
{
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = ServerHandle { sender };
    let extension = create(handle.clone());
    let address = address.to_socket_addrs().unwrap().next().unwrap();
    let future = tokio::spawn({
        async move {
            let mut actor = ServerActor {
                receiver,
                address,
                extension,
            };
            actor.run().await?;
            Ok::<_, BoxError>(())
        }
    });
    let future = async move { future.await.unwrap() };
    (handle, future)
}
