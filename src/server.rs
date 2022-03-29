use crate::BoxError;
use crate::Session;
use crate::SessionExt;
use crate::Socket;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use tokio::sync::mpsc;

struct ServerActor<E: ServerExt> {
    connections: mpsc::UnboundedReceiver<(Socket, SocketAddr)>,
    calls: mpsc::UnboundedReceiver<E::Params>,
    extension: E,
}

impl<E: ServerExt> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as SessionExt>::ID: Send,
{
    async fn run(&mut self) -> Result<(), BoxError> {
        tracing::info!("starting server");
        loop {
            tokio::select! {
                Some((socket, address)) = self.connections.recv() => {
                    self.extension.accept(socket, address).await?;
                    tracing::info!("connection from {address} accepted");
                }
                Some(params) = self.calls.recv() => {
                    self.extension.call(params).await?
                }
                else => break
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait ServerExt: Send {
    type Session: SessionExt;
    type Params: Send + std::fmt::Debug;

    async fn accept(&mut self, socket: Socket, address: SocketAddr) -> Result<Session, BoxError>;
    async fn disconnected(&mut self, id: <Self::Session as SessionExt>::ID)
        -> Result<(), BoxError>;
    async fn call(&mut self, params: Self::Params) -> Result<(), BoxError>;
}

#[derive(Debug)]
pub struct Server<P: std::fmt::Debug = ()> {
    connections: mpsc::UnboundedSender<(Socket, SocketAddr)>,
    calls: mpsc::UnboundedSender<P>,
}

impl<P: std::fmt::Debug + Send> Server<P> {
    pub async fn create<E: ServerExt<Params = P> + 'static>(
        create: impl FnOnce(Self) -> E,
    ) -> (Self, impl Future<Output = Result<(), BoxError>>) {
        let (connection_sender, connection_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();
        let handle = Self {
            connections: connection_sender,
            calls: call_sender,
        };
        let extension = create(handle.clone());
        let mut actor = ServerActor {
            connections: connection_receiver,
            calls: call_receiver,
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

impl<P: std::fmt::Debug> Server<P> {
    pub async fn accept(&self, socket: Socket, address: SocketAddr) {
        self.connections
            .send((socket, address))
            .map_err(|_| ())
            .unwrap();
    }

    pub async fn call(&self, params: P) {
        self.calls.send(params).map_err(|_| ()).unwrap();
    }
}

impl<P: std::fmt::Debug> std::clone::Clone for Server<P> {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            calls: self.calls.clone(),
        }
    }
}
