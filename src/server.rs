use crate::BoxError;
use crate::CloseFrame;
use crate::Message;
use async_trait::async_trait;
use futures::Future;
use futures::SinkExt;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use tracing::Instrument;
use tracing::Span;

type WebSocketStream = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

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
        let handle = SessionHandle { sender };
        let extension = self.extension.connected(handle, address).await?;
        let id = extension.id().clone();
        tracing::info!(%id, %address, "connected");
        let mut actor = SessionActor {
            extension,
            receiver,
            stream: websocket_stream,
            heartbeat: Instant::now(),
        };
        tokio::spawn(
            async move {
                let result = actor.run().await;
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

#[async_trait]
pub trait Session: Send {
    type ID: Clone + std::fmt::Debug + std::fmt::Display;

    fn id(&self) -> &Self::ID;
    async fn text(&mut self, text: String) -> Result<Option<Message>, BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<Message>, BoxError>;
    async fn disconnected(&mut self) -> Result<(), BoxError>;
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    sender: mpsc::UnboundedSender<Message>,
}

impl SessionHandle {
    pub async fn text(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }

    pub async fn binary(&self, text: String) {
        self.sender.send(Message::Text(text)).unwrap();
    }
}

struct SessionActor<E: Session> {
    extension: E,
    receiver: mpsc::UnboundedReceiver<Message>,
    stream: WebSocketStream,
    heartbeat: Instant,
}

impl<S: Session> SessionActor<S> {
    async fn run(&mut self) -> Result<Option<CloseFrame>, BoxError> {
        use futures::StreamExt;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    let message = match message {
                        Message::Text(text) => tungstenite::Message::Text(text),
                        Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
                        Message::Close(frame) => {
                            return Ok(frame);
                        }
                    };
                    self.stream.send(message).await?;
                }
                Some(message) = self.stream.next() => {
                    let message = message?;
                    let message = match message {
                        tungstenite::Message::Text(text) => self.extension.text(text).await?,
                        tungstenite::Message::Binary(bytes) => self.extension.binary(bytes).await?,
                        tungstenite::Message::Ping(bytes) => {
                            self.stream.send(tungstenite::Message::Pong(bytes)).await?;
                            None
                        }
                        tungstenite::Message::Pong(_) => {
                            // TODO: Maybe handle bytes?
                            self.heartbeat = Instant::now();
                            None
                        }
                        tungstenite::Message::Close(frame) => {
                            return Ok(frame.map(CloseFrame::from))
                        }
                        tungstenite::Message::Frame(_) => todo!(),
                    };
                    if let Some(message) = message {
                        self.stream.send(message.into()).await?;
                    }
                }
                _ = interval.tick() => {
                    self.stream.send(tungstenite::Message::Ping(Vec::new())).await?;
                }
                else => break,
            }
        }

        Ok(None)
    }
}
