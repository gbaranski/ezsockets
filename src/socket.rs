use crate::Error;
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<CloseFrame>),
}

pub struct Sink {
    sender: Box<dyn FnMut(Message) -> Result<(), Error> + Send + Sync>,
}

impl Sink {
    pub fn send(&mut self, message: Message) -> Result<(), Error> {
        (self.sender)(message)
    }
}

#[derive(Debug)]
struct StreamImpl<M, S>
where
    M: Into<Message>,
    S: StreamExt<Item = Result<M, Error>> + Unpin,
{
    stream: S,
    last_alive: Arc<std::sync::Mutex<Instant>>,
}
impl<M, S> futures::Stream for StreamImpl<M, S>
where
    M: Into<Message>,
    S: StreamExt<Item = Result<M, Error>> + Unpin,
{
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let message = match ready!(self.stream.poll_next_unpin(cx)) {
            Some(x) => x?.into(),
            None => return Poll::Ready(None),
        };
        if let Message::Pong(bytes) = &message {
            *self.last_alive.lock().unwrap() = Instant::now();
            if let Ok(bytes) = bytes.clone().try_into() {
                let bytes: [u8; 16] = bytes;
                let timestamp = u128::from_be_bytes(bytes);
                let timestamp = Duration::from_millis(timestamp as u64); // TODO: handle overflow
                let latency = SystemTime::now()
                    .duration_since(UNIX_EPOCH + timestamp)
                    .unwrap();
                // TODO: handle time zone
                tracing::trace!("latency: {}ms", latency.as_millis());
            }
        }
        Poll::Ready(Some(Ok(message)))
    }
}

pub struct Stream {
    stream_impl: BoxStream<'static, Result<Message, Error>>,
}

impl Stream {
    fn new<M, S>(stream: S, last_alive: Arc<std::sync::Mutex<Instant>>) -> Self
    where
        M: Into<Message> + std::fmt::Debug + Send + 'static,
        S: StreamExt<Item = Result<M, Error>> + Unpin + Send + 'static,
    {
        let stream_impl = StreamImpl { stream, last_alive };

        Self {
            stream_impl: stream_impl.boxed(),
        }
    }

    pub async fn recv(&mut self) -> Option<Result<Message, Error>> {
        self.stream_impl.next().await
    }
}

pub struct Socket {
    pub sink: Sink,
    pub stream: Stream,
    pub config: Config,
}

impl Socket {
    pub fn new<M, E: std::error::Error, S>(socket: S, config: Config) -> Self
    where
        M: Into<Message> + From<Message> + std::fmt::Debug + Send + Sync + 'static,
        E: Into<Error>,
        S: SinkExt<M, Error = E> + Unpin + StreamExt<Item = Result<M, E>> + Unpin + Send + 'static,
    {
        let last_alive = Instant::now();
        let last_alive = Arc::new(std::sync::Mutex::new(last_alive));
        let (mut sink, stream) = socket.sink_err_into().err_into().split();
        let (sink, stream) = (
            Sink {
                sender: Box::new(move |x| sink.start_send_unpin(x.into())),
            },
            Stream::new(stream, last_alive.clone()),
        );

        Self {
            sink,
            stream,
            config,
        }
    }
}
