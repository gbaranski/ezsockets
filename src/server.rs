use crate::BoxError;
use crate::Session;
use crate::SessionHandle;
use crate::WebSocket;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use tokio::sync::mpsc;

#[derive(Debug)]
enum ServerMessage<E: Server> {
    Accept {
        socket: WebSocket,
        address: SocketAddr,
    },
    Message(E::Message),
}

struct ServerActor<E: Server> {
    receiver: mpsc::UnboundedReceiver<ServerMessage<E>>,
    extension: E,
    address: SocketAddr,
}

impl<E: Server> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as Session>::ID: Send,
{
    async fn run(&mut self) -> Result<(), BoxError> {
        tracing::info!("starting server on {}", self.address);
        let listener = tokio::net::TcpListener::bind(self.address).await?;
        loop {
            tokio::select! {
                Ok((socket, address)) = listener.accept() => {
                    let socket = tokio_tungstenite::accept_async(socket).await?;
                    let socket = WebSocket::new(socket);
                    self.accept(socket, address).await?;
                }
                Some(message) = self.receiver.recv() => {
                    match message {
                        ServerMessage::Accept { socket, address } => { self.accept(socket, address).await?; },
                        ServerMessage::Message(message) => self.extension.message(message).await,
                    };
                }
                else => break,
            }
        }
        Ok(())
    }

    async fn accept(&mut self, socket: WebSocket, address: SocketAddr) -> Result<(), BoxError> {
        self.extension.accept(socket, address).await?;
        tracing::info!("connection from {address} accepted");
        Ok(())
    }
}

#[async_trait]
pub trait Server: Send {
    type Session: Session;
    type Message: Send;

    async fn accept(
        &mut self,
        socket: WebSocket,
        address: SocketAddr,
    ) -> Result<SessionHandle, BoxError>;
    async fn message(&mut self, message: Self::Message);
}

#[derive(Debug)]
pub struct ServerHandle<E: Server> {
    sender: mpsc::UnboundedSender<ServerMessage<E>>,
}

impl<E: Server> ServerHandle<E>
where
    E::Message: std::fmt::Debug,
{
    pub async fn accept(&self, socket: WebSocket, address: SocketAddr) {
        self.sender
            .send(ServerMessage::Accept { socket, address })
            .map_err(|_| ())
            .unwrap();
    }

    pub async fn call(&self, message: E::Message) {
        self.sender
            .send(ServerMessage::Message(message))
            .map_err(|_| ())
            .unwrap();
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
