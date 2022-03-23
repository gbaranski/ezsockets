use crate::BoxError;
use crate::SessionExt;
use crate::SessionHandle;
use crate::Socket;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use tokio::sync::mpsc;

#[derive(Debug)]
enum ServerMessage<M> {
    Accept { socket: Socket, address: SocketAddr },
    Message(M),
}

struct ServerActor<E: ServerExt> {
    receiver: mpsc::UnboundedReceiver<ServerMessage<E::Message>>,
    extension: E,
}

impl<E: ServerExt> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as SessionExt>::ID: Send,
{
    async fn run(&mut self) -> Result<(), BoxError> {
        tracing::info!("starting server");
        while let Some(message) = self.receiver.recv().await {
            match message {
                ServerMessage::Accept { socket, address } => {
                    self.accept(socket, address).await?;
                }
                ServerMessage::Message(message) => self.extension.message(message).await,
            };
        }

        Ok(())
    }

    async fn accept(&mut self, socket: Socket, address: SocketAddr) -> Result<(), BoxError> {
        self.extension.accept(socket, address).await?;
        tracing::info!("connection from {address} accepted");
        Ok(())
    }
}

#[async_trait]
pub trait ServerExt: Send {
    type Session: SessionExt;
    type Message: Send;

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
    ) -> Result<SessionHandle, BoxError>;
    async fn disconnected(&mut self, id: <Self::Session as SessionExt>::ID) -> Result<(), BoxError>;
    async fn message(&mut self, message: Self::Message);
}

#[derive(Debug)]
pub struct Server<E: ServerExt> {
    sender: mpsc::UnboundedSender<ServerMessage<E::Message>>,
}

impl<E> Server<E>
where
    E: ServerExt + 'static,
    E::Message: std::fmt::Debug,
{
    pub async fn create(
        create: impl FnOnce(Self) -> E,
    ) -> (Self, impl Future<Output = Result<(), BoxError>>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let handle = Server { sender };
        let extension = create(handle.clone());
        let mut actor = ServerActor {
            receiver,
            extension,
        };
        let future = tokio::spawn(async move {
            actor.run().await?;
            Ok::<_, BoxError>(())
        });
        let future = async move { future.await.unwrap() };
        (handle, future)
    }
}

impl<E> Server<E>
where
    E: ServerExt,
{
    pub async fn accept(&self, socket: Socket, address: SocketAddr) {
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

impl<E: ServerExt> std::clone::Clone for Server<E> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}
