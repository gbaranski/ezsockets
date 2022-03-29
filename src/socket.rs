use crate::Error;
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::sync::Arc;
use std::time::Instant;
use std::{
    marker::PhantomData,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Config {
    pub heartbeat: Duration,
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
        }
    }
}

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
}

impl From<CloseCode> for u16 {
    fn from(code: CloseCode) -> u16 {
        use self::CloseCode::*;
        match code {
            Normal => 1000,
            Away => 1001,
            Protocol => 1002,
            Unsupported => 1003,
            Status => 1005,
            Abnormal => 1006,
            Invalid => 1007,
            Policy => 1008,
            Size => 1009,
            Extension => 1010,
            Error => 1011,
            Restart => 1012,
            Again => 1013,
        }
    }
}

impl TryFrom<u16> for CloseCode {
    type Error = u16;

    fn try_from(code: u16) -> Result<Self, u16> {
        use self::CloseCode::*;

        Ok(match code {
            1000 => Normal,
            1001 => Away,
            1002 => Protocol,
            1003 => Unsupported,
            1005 => Status,
            1006 => Abnormal,
            1007 => Invalid,
            1008 => Policy,
            1009 => Size,
            1010 => Extension,
            1011 => Error,
            1012 => Restart,
            1013 => Again,
            code => {
                return Err(code);
            }
        })
    }
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
    S: SinkExt<M, Error = Error> + Unpin,
{
    receiver: mpsc::UnboundedReceiver<RawMessage>,
    sink: S,
    phantom: PhantomData<M>,
}

impl<M, S> SinkActor<M, S>
where
    M: From<RawMessage>,
    S: SinkExt<M, Error = Error> + Unpin,
{
    async fn run(&mut self) -> Result<(), Error> {
        while let Some(message) = self.receiver.recv().await {
            tracing::trace!("sending message: {:?}", message);
            self.sink.send(M::from(message)).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Sink {
    sender: mpsc::UnboundedSender<RawMessage>,
}

impl Sink {
    fn new<M, S>(sink: S) -> (tokio::task::JoinHandle<Result<(), Error>>, Self)
    where
        M: From<RawMessage> + Send + 'static,
        S: SinkExt<M, Error = Error> + Unpin + Send + 'static,
    {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut actor = SinkActor {
            receiver,
            sink,
            phantom: Default::default(),
        };
        let future = tokio::spawn(async move { actor.run().await });
        (future, Self { sender })
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    pub async fn send(&self, message: Message) {
        self.sender.send(message.into()).unwrap();
    }

    pub(crate) async fn send_raw(&self, message: RawMessage) {
        self.sender.send(message).unwrap();
    }
}

#[derive(Debug)]
struct StreamActor<M, S>
where
    M: Into<RawMessage>,
    S: StreamExt<Item = Result<M, Error>> + Unpin,
{
    sender: mpsc::UnboundedSender<Message>,
    stream: S,
    last_alive: Arc<Mutex<Instant>>,
}

impl<M, S> StreamActor<M, S>
where
    M: Into<RawMessage>,
    S: StreamExt<Item = Result<M, Error>> + Unpin,
{
    async fn run(&mut self) -> Result<(), Error> {
        while let Some(message) = self.stream.next().await.transpose()? {
            let message: RawMessage = message.into();
            tracing::trace!("received message: {:?}", message);
            let message = match message {
                RawMessage::Text(text) => Message::Text(text),
                RawMessage::Binary(bytes) => Message::Binary(bytes),
                RawMessage::Ping(_bytes) => continue,
                RawMessage::Pong(bytes) => {
                    *self.last_alive.lock().await = Instant::now();
                    let bytes: [u8; 16] = bytes.try_into().unwrap(); // TODO: handle invalid byte frame
                    let timestamp = u128::from_be_bytes(bytes);
                    let timestamp = Duration::from_millis(timestamp as u64); // TODO: handle overflow
                    let latency = SystemTime::now()
                        .duration_since(UNIX_EPOCH + timestamp)
                        .unwrap();
                    tracing::trace!("latency: {}ms", latency.as_millis());
                    continue;
                }
                RawMessage::Close(_) => return Ok(()),
            };
            self.sender.send(message).unwrap();
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Stream {
    receiver: mpsc::UnboundedReceiver<Message>,
}

impl Stream {
    fn new<M, S>(
        stream: S,
        last_alive: Arc<Mutex<Instant>>,
    ) -> (tokio::task::JoinHandle<Result<(), Error>>, Self)
    where
        M: Into<RawMessage> + std::fmt::Debug + Send + 'static,
        S: StreamExt<Item = Result<M, Error>> + Unpin + Send + 'static,
    {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut actor = StreamActor {
            sender,
            stream,
            last_alive,
        };
        let future = tokio::spawn(async move { actor.run().await });
        (future, Self { receiver })
    }

    pub async fn recv(&mut self) -> Option<Message> {
        self.receiver.recv().await
    }
}

#[derive(Debug)]
pub struct Socket {
    pub sink: Sink,
    pub stream: Stream,
}

impl Socket {
    pub fn new<M, E: std::error::Error, S>(socket: S, config: Config) -> Self
    where
        M: Into<RawMessage> + From<RawMessage> + std::fmt::Debug + Send + 'static,
        E: Into<Error>,
        S: SinkExt<M, Error = E> + Unpin + StreamExt<Item = Result<M, E>> + Unpin + Send + 'static,
    {
        let last_alive = Instant::now();
        let last_alive = Arc::new(Mutex::new(last_alive));
        let (sink, stream) = socket.sink_err_into().err_into().split();
        let ((sink_future, sink), (stream_future, stream)) =
            (Sink::new(sink), Stream::new(stream, last_alive.clone()));
        let heartbeat_future = tokio::spawn({
            let sink = sink.clone();
            async move {
                let mut interval = tokio::time::interval(config.heartbeat);

                loop {
                    interval.tick().await;
                    if last_alive.lock().await.elapsed() > config.timeout {
                        tracing::info!("closing connection due to timeout");
                        sink.send_raw(RawMessage::Close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: String::from("client didn't respond to Ping frame"),
                        })))
                        .await;
                        return;
                    }
                    let timestamp = SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap();
                    let timestamp = timestamp.as_millis();
                    let bytes = timestamp.to_be_bytes();
                    sink.send_raw(RawMessage::Ping(bytes.to_vec())).await;
                }
            }
        });

        tokio::spawn(async move {
            let result = stream_future.await.unwrap();
            sink_future.abort();
            heartbeat_future.abort();
            match &result {
                Ok(_) => tracing::info!("connection closed"),
                Err(error) => tracing::info!("connection failed with error: {error}"),
            };
            result
        });

        Self {
            sink,
            stream,
        }
    }

    pub async fn send(&self, message: Message) {
        self.sink.send(message).await;
    }

    pub async fn send_raw(&self, message: RawMessage) {
        self.sink.send_raw(message).await;
    }

    pub async fn recv(&mut self) -> Option<Message> {
        self.stream.recv().await
    }
}
