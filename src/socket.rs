use crate::BoxError;
use futures::{SinkExt,  StreamExt, TryStreamExt};
use std::marker::PhantomData;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;


#[derive(Debug, Clone)]
pub enum CloseCode {
    /// Indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    Normal,
    /// Indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    Away,
    /// Indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    Protocol,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    Unsupported,
    /// Indicates that no status code was included in a closing frame. This
    /// close code makes it possible to use a single method, `on_close` to
    /// handle even cases where no close code was provided.
    Status,
    /// Indicates an abnormal closure. If the abnormal closure was due to an
    /// error, this close code will not be used. Instead, the `on_error` method
    /// of the handler will be called with the error. However, if the connection
    /// is simply dropped, without an error, this close code will be sent to the
    /// handler.
    Abnormal,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    Invalid,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., Unsupported or Size) or if there
    /// is a need to hide specific details about the policy.
    Policy,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    Size,
    /// Indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed should be given as the reason for closing.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    Extension,
    /// Indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    Error,
    /// Indicates that the server is restarting. A client may choose to reconnect,
    /// and if it does, it should use a randomized delay of 5-30 seconds between attempts.
    Restart,
    /// Indicates that the server is overloaded and the client should either connect
    /// to a different IP (when multiple targets exist), or reconnect to the same IP
    /// when a user has performed an action.
    Again,
    #[doc(hidden)]
    Tls,
    #[doc(hidden)]
    Reserved(u16),
    #[doc(hidden)]
    Iana(u16),
    #[doc(hidden)]
    Library(u16),
    #[doc(hidden)]
    Bad(u16),
}

#[derive(Debug, Clone)]
pub struct CloseFrame {
    pub code: CloseCode,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<CloseFrame>),
}

impl From<Message> for tungstenite::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => tungstenite::Message::Text(text),
            Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
            Message::Close(frame) => tungstenite::Message::Close(frame.map(CloseFrame::into)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RawMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<CloseFrame>),
}

impl From<Message> for RawMessage {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => Self::Text(text),
            Message::Binary(bytes) => Self::Binary(bytes),
            Message::Close(frame) => Self::Close(frame.map(CloseFrame::from)),
        }
    }
}



#[derive(Debug)]
struct SinkActor<M, S>
where
    M: From<RawMessage>,
    S: SinkExt<M, Error = BoxError> + Unpin,
{
    receiver: mpsc::UnboundedReceiver<RawMessage>,
    sink: S,
    phantom: PhantomData<M>,
}

impl<M, S> SinkActor<M, S>
where
    M: From<RawMessage>,
    S: SinkExt<M, Error = BoxError> + Unpin,
{
    async fn run(&mut self) -> Result<(), BoxError> {
        while let Some(message) = self.receiver.recv().await {
            tracing::debug!("sending message: {:?}", message);
            self.sink.send(M::from(message)).await?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Sink {
    sender: mpsc::UnboundedSender<RawMessage>,
}

impl Sink {
    pub fn new<M, S>(sink: S) -> Self
    where
        M: From<RawMessage> + Send + 'static,
        S: SinkExt<M, Error = BoxError> + Unpin + Send + 'static,
    {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut actor = SinkActor {
            receiver,
            sink,
            phantom: Default::default(),
        };
        tokio::spawn(async move { actor.run().await });
        Self { sender }
    }

    pub async fn send(&self, message: RawMessage) {
        self.sender.send(message).unwrap();
    }
}

#[derive(Debug)]
struct StreamActor<M, S>
where
    M: Into<RawMessage>,
    S: StreamExt<Item = Result<M, BoxError>> + Unpin,
{
    sender: mpsc::UnboundedSender<RawMessage>,
    stream: S,
}

impl<M, S> StreamActor<M, S>
where
    M: Into<RawMessage> + std::fmt::Debug,
    S: StreamExt<Item = Result<M, BoxError>> + Unpin,
{
    async fn run(&mut self) -> Result<(), BoxError> {
        while let Some(message) = self.stream.next().await.transpose()? {
            let message = message.into();
            tracing::debug!("received message: {:?}", message);
            self.sender.send(message).unwrap();
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Stream {
    pub future: tokio::task::JoinHandle<Result<(), BoxError>>,
    receiver: mpsc::UnboundedReceiver<RawMessage>,
}

impl Stream {
    pub fn new<M, S>(stream: S) -> Self
    where
        M: Into<RawMessage> + std::fmt::Debug + Send + 'static,
        S: StreamExt<Item = Result<M, BoxError>> + Unpin + Send + 'static,
    {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut actor = StreamActor { sender, stream };
        let future = tokio::spawn(async move { actor.run().await });
        Self { future, receiver }
    }

    pub async fn recv(&mut self) -> Option<RawMessage> {
        self.receiver.recv().await
    }
}

#[derive(Debug)]
pub struct Socket {
    pub sink: Sink,
    pub stream: Stream,
}

impl Socket {
    pub fn new<M, E: std::error::Error, S>(socket: S) -> Self
    where
        M: Into<RawMessage> + From<RawMessage> + std::fmt::Debug + Send + 'static,
        E: Into<BoxError>,
        S: SinkExt<M, Error = E> + Unpin + StreamExt<Item = Result<M, E>> + Unpin + Send + 'static,
    {
        let (sink, stream) = socket.sink_err_into().err_into().split();
        let (sink, stream) = (Sink::new(sink), Stream::new(stream));
        Self { sink: sink, stream }
    }

    pub async fn send(&self, message: RawMessage) {
        self.sink.send(message).await;
    }

    pub async fn recv(&mut self) -> Option<RawMessage> {
        self.stream.recv().await
    }
}
