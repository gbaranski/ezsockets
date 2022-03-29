use crate::CloseFrame;
use crate::Error;
use crate::Session;
use crate::SessionExt;
use crate::Socket;
use async_trait::async_trait;
use futures::Future;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

struct ServerActor<E: ServerExt> {
    connections: mpsc::UnboundedReceiver<(
        Socket,
        SocketAddr,
        <E::Session as SessionExt>::Args,
        oneshot::Sender<<E::Session as SessionExt>::ID>,
    )>,
    disconnections: mpsc::UnboundedReceiver<(<E::Session as SessionExt>::ID, Result<Option<CloseFrame>, Error>)>,
    calls: mpsc::UnboundedReceiver<E::Params>,
    server: Server<E>,
    extension: E,
}

impl<E: ServerExt> ServerActor<E>
where
    E: Send + 'static,
    <E::Session as SessionExt>::ID: Send,
{
    async fn run(&mut self) -> Result<(), Error> {
        tracing::info!("starting server");
        loop {
            tokio::select! {
                Some((socket, address, args, respond_to)) = self.connections.recv() => {
                    let session = self.extension.accept(socket, address, args).await?;
                    let session_id = session.id.clone();
                    tracing::info!("connection from {address} accepted");
                    respond_to.send(session_id.clone()).unwrap();

                    tokio::spawn({
                        let server = self.server.clone();
                        async move {
                            let result = session.closed().await;
                            server.disconnected(session_id, result).await;
                        }
                    });
                }
                Some((id, result)) = self.disconnections.recv() => {
                    self.extension.disconnected(id.clone()).await?;
                    match result {
                        Ok(Some(CloseFrame { code, reason })) => {
                            tracing::info!(%id, ?code, %reason, "connection closed")
                        }
                        Ok(None) => tracing::info!(%id, "connection closed"),
                        Err(err) => tracing::warn!(%id, "connection closed due to: {err}"),
                    };
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

    async fn accept(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        args: <Self::Session as SessionExt>::Args,
    ) -> Result<
        Session<<Self::Session as SessionExt>::ID, <Self::Session as SessionExt>::Params>,
        Error,
    >;
    async fn disconnected(&mut self, id: <Self::Session as SessionExt>::ID) -> Result<(), Error>;
    async fn call(&mut self, params: Self::Params) -> Result<(), Error>;
}

#[derive(Debug)]
pub struct Server<E: ServerExt> {
    connections: mpsc::UnboundedSender<(
        Socket,
        SocketAddr,
        <E::Session as SessionExt>::Args,
        oneshot::Sender<<E::Session as SessionExt>::ID>,
    )>,
    disconnections: mpsc::UnboundedSender<(<E::Session as SessionExt>::ID, Result<Option<CloseFrame>, Error>)>,
    calls: mpsc::UnboundedSender<E::Params>,
}

impl<E: ServerExt> From<Server<E>> for mpsc::UnboundedSender<E::Params> {
    fn from(server: Server<E>) -> Self {
        server.calls
    }
}

impl<E: ServerExt + 'static> Server<E> {
    pub fn create(
        create: impl FnOnce(Self) -> E,
    ) -> (Self, impl Future<Output = Result<(), Error>>) {
        let (connection_sender, connection_receiver) = mpsc::unbounded_channel();
        let (disconnection_sender, disconnection_receiver) = mpsc::unbounded_channel();
        let (call_sender, call_receiver) = mpsc::unbounded_channel();
        let handle = Self {
            connections: connection_sender,
            calls: call_sender,
            disconnections: disconnection_sender,
        };
        let extension = create(handle.clone());
        let mut actor = ServerActor {
            connections: connection_receiver,
            disconnections: disconnection_receiver,
            calls: call_receiver,
            extension,
            server: handle.clone(),
        };
        let future = tokio::spawn(async move {
            actor.run().await?;
            Ok::<_, Error>(())
        });
        let future = async move { future.await.unwrap() };
        (handle, future)
    }
}

impl<E: ServerExt> Server<E> {
    pub async fn accept(
        &self,
        socket: Socket,
        address: SocketAddr,
        args: <E::Session as SessionExt>::Args,
    ) -> <E::Session as SessionExt>::ID {
        let (sender, receiver) = oneshot::channel();
        self.connections
            .send((socket, address, args, sender))
            .map_err(|_| ())
            .unwrap();
        let session_id = receiver.await.unwrap();
        session_id
    }

    pub(crate) async fn disconnected(&self, id: <E::Session as SessionExt>::ID, result: Result<Option<CloseFrame>, Error>) {
        self.disconnections.send((id, result)).map_err(|_| ()).unwrap();
    }

    pub async fn call(&self, params: E::Params) {
        self.calls.send(params).map_err(|_| ()).unwrap();
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the Params which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> E::Params,
    ) -> R {
        let (sender, receiver) = oneshot::channel();
        let params = f(sender);

        self.calls.send(params).unwrap();
        let response = receiver.await.unwrap();

        response
    }
}

impl<E: ServerExt> std::clone::Clone for Server<E> {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            disconnections: self.disconnections.clone(),
            calls: self.calls.clone(),
        }
    }
}
