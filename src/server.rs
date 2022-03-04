use crate::BoxError;
use crate::CloseCode;
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
{
    async fn run(mut self) -> Result<(), BoxError> {
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
        let session = self.extension.connected(handle, address).await?;
        let actor = SessionActor {
            inner: session,
            receiver,
            stream: websocket_stream,
            heartbeat: Instant::now(),
        };
        tokio::spawn(
            async move {
                let result = actor.run().await;
                match result {
                    Ok(_) => tracing::info!("connection closed"),
                    Err(err) => tracing::warn!("connection error: {err}"),
                }
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
pub struct ServerHandle<M> {
    sender: mpsc::UnboundedSender<M>,
}

impl<M: std::fmt::Debug> ServerHandle<M> {
    pub async fn call(&self, message: M) {
        self.sender.send(message).unwrap();
    }
}

impl<M> std::clone::Clone for ServerHandle<M> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

pub async fn run<E>(
    create: impl FnOnce(ServerHandle<E::Message>) -> E,
    address: impl ToSocketAddrs,
) -> (
    ServerHandle<E::Message>,
    impl Future<Output = Result<(), BoxError>>,
)
where
    E: Server + 'static,
    E::Message: Send + 'static,
{
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = ServerHandle { sender };
    let extension = create(handle.clone());
    let address = address.to_socket_addrs().unwrap().next().unwrap();
    let future = tokio::spawn({
        async move {
            let actor = ServerActor {
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
    async fn text(&mut self, text: String) -> Result<Option<Message>, BoxError>;
    async fn binary(&mut self, bytes: Vec<u8>) -> Result<Option<Message>, BoxError>;
    async fn closed(
        &mut self,
        code: Option<CloseCode>,
        reason: Option<String>,
    ) -> Result<(), BoxError>;
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

struct SessionActor<S: Session> {
    inner: S,
    receiver: mpsc::UnboundedReceiver<Message>,
    stream: WebSocketStream,
    heartbeat: Instant,
}

impl<S: Session> SessionActor<S> {
    async fn run(mut self) -> Result<(), BoxError> {
        use futures::StreamExt;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    let should_continue = self.handle_message(message).await?;
                    if !should_continue {
                        return Ok(())
                    }
                }
                Some(message) = self.stream.next() => {
                    let message = message?;
                    let should_continue = self.handle_websocket_message(message).await?;
                    if !should_continue {
                        return Ok(())
                    }
                }
                _ = interval.tick() => {
                    self.stream.send(tungstenite::Message::Ping(Vec::new())).await?;
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> Result<bool, BoxError> {
        let message = match message {
            Message::Text(text) => tungstenite::Message::Text(text),
            Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
            Message::Close(frame) => {
                let frame = frame.map(|(code, reason)| tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                });
                self.stream.close(frame).await?;
                return Ok(false);
            }
        };
        self.stream.send(message).await?;
        Ok(true)
    }

    async fn handle_websocket_message(
        &mut self,
        message: tungstenite::Message,
    ) -> Result<bool, BoxError> {
        let message = match message {
            tungstenite::Message::Text(text) => self.inner.text(text).await?,
            tungstenite::Message::Binary(bytes) => self.inner.binary(bytes).await?,
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
                let (code, reason) = if let Some(frame) = frame {
                    (Some(frame.code.into()), Some(frame.reason.into()))
                } else {
                    (None, None)
                };
                self.inner.closed(code, reason).await?;
                None
            }
            tungstenite::Message::Frame(_) => todo!(),
        };
        if let Some(message) = message {
            self.stream.send(message.into()).await?;
        }
        Ok(true)
    }
}
