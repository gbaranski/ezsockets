use futures::lock::Mutex;
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite_wasm::Error as WSError;

#[cfg(not(target_family = "wasm"))]
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_family = "wasm")]
use wasmtimer::std::{Instant, SystemTime, UNIX_EPOCH};

/// Wrapper trait for `Fn(Duration) -> RawMessage`.
pub trait SocketHeartbeatPingFn: Fn(Duration) -> RawMessage + Sync + Send {}
impl<F> SocketHeartbeatPingFn for F where F: Fn(Duration) -> RawMessage + Sync + Send {}
pub type SocketHeartbeatPingFnT = dyn SocketHeartbeatPingFn<Output = RawMessage>;

impl std::fmt::Debug for SocketHeartbeatPingFnT {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

/// Socket configuration.
#[derive(Debug, Clone)]
pub struct SocketConfig {
    /// Duration between each heartbeat check.
    pub heartbeat: Duration,
    /// Duration before the keep-alive will fail if there are no new stream messages.
    pub timeout: Duration,
    /// Convert 'current time as duration since UNIX_EPOCH' into a ping message (on heartbeat).
    /// This may be useful for manually implementing Ping/Pong messages via `RawMessage::Text` or `RawMessage::Binary`
    /// if Ping/Pong are not available for your socket (e.g. in browser).
    /// The default function outputs a standard `RawMessage::Ping`, with the payload set to the timestamp in milliseconds in
    /// big-endian bytes.
    pub heartbeat_ping_msg_fn: Arc<dyn SocketHeartbeatPingFn>,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
            heartbeat_ping_msg_fn: Arc::new(|timestamp: Duration| {
                let timestamp = timestamp.as_millis();
                let bytes = timestamp.to_be_bytes();
                RawMessage::Ping(bytes.to_vec())
            }),
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
            Tls => 1015,
            Reserved(code) => code,
            Iana(code) => code,
            Library(code) => code,
            Bad(code) => code,
        }
    }
}

impl From<u16> for CloseCode {
    fn from(code: u16) -> Self {
        use self::CloseCode::*;

        match code {
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
            1015 => Tls,
            1..=999 => Bad(code),
            1016..=2999 => Reserved(code),
            3000..=3999 => Iana(code),
            4000..=4999 => Library(code),
            _ => Bad(code),
        }
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
    Ping(Vec<u8>),
    Pong(Vec<u8>),
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
            Message::Ping(bytes) => Self::Ping(bytes),
            Message::Pong(bytes) => Self::Pong(bytes),
            Message::Close(frame) => Self::Close(frame.map(CloseFrame::from)),
        }
    }
}

/// Possible states of a submitted message.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MessageStatus {
    /// Message is in the process of being sent.
    Sending,
    /// Message was successfully sent.
    Sent,
    /// Message failed sending.
    Failed,
}

/// Signal that listens to the current `MessageStatus` of a submitted message.
#[derive(Debug, Clone)]
pub struct MessageSignal {
    signal: Arc<AtomicU8>,
}

impl MessageSignal {
    /// Makes a new `MessageSignal` that starts with the specified status.
    ///
    /// Useful for creating [`MessageStatus::Failed`] statuses without actually trying to send a message.
    pub fn new(status: MessageStatus) -> Self {
        let signal = Self::default();
        signal.set(status);
        signal
    }

    /// Reads the signal's [`MessageStatus`].
    pub fn status(&self) -> MessageStatus {
        match self.signal.load(Ordering::Acquire) {
            0u8 => MessageStatus::Sending,
            1u8 => MessageStatus::Sent,
            _ => MessageStatus::Failed,
        }
    }

    /// Sets the signal's [`MessageStatus`].
    ///
    /// This is crate-private so clones of a signal in the wild cannot lie to each other.
    pub(crate) fn set(&self, status: MessageStatus) {
        match status {
            MessageStatus::Sending => self.signal.store(0u8, Ordering::Release),
            MessageStatus::Sent => self.signal.store(1u8, Ordering::Release),
            MessageStatus::Failed => self.signal.store(2u8, Ordering::Release),
        }
    }
}

impl Default for MessageSignal {
    fn default() -> Self {
        Self {
            signal: Arc::new(AtomicU8::new(0u8)),
        }
    }
}

/// Raw message with associated message signal.
#[derive(Debug, Clone)]
pub struct InRawMessage {
    /// We use an `Option` for the message so that we can both extract messages for sending and implement `Drop`.
    message: Option<RawMessage>,
    signal: Option<MessageSignal>,
}

impl InRawMessage {
    pub fn new(message: RawMessage) -> Self {
        Self {
            message: Some(message),
            signal: Some(MessageSignal::default()),
        }
    }

    pub(crate) fn take_message(&mut self) -> Option<RawMessage> {
        self.message.take()
    }

    pub(crate) fn set_signal(&mut self, state: MessageStatus) {
        let Some(signal) = &self.signal else {
            return;
        };
        signal.set(state);
        self.signal = None;
    }
}

impl Drop for InRawMessage {
    fn drop(&mut self) {
        // If the signal is still present in the message when dropping, then we need to mark its state as failed.
        self.set_signal(MessageStatus::Failed);
    }
}

/// Message with associated message signal.
#[derive(Debug, Clone)]
pub struct InMessage {
    /// We use an `Option` for the message so that we can both convert to `InMessage`s and implement `Drop`.
    pub(crate) message: Option<Message>,
    signal: Option<MessageSignal>,
}

impl InMessage {
    pub fn new(message: Message) -> Self {
        Self {
            message: Some(message),
            signal: Some(MessageSignal::default()),
        }
    }

    pub fn clone_signal(&self) -> Option<MessageSignal> {
        self.signal.clone()
    }
}

impl From<InMessage> for InRawMessage {
    fn from(mut inmessage: InMessage) -> Self {
        Self {
            message: inmessage.message.take().map(|msg| msg.into()),
            signal: inmessage.signal.take(),
        }
    }
}

impl Drop for InMessage {
    fn drop(&mut self) {
        // If the signal is still present in the message when dropping, then we need to mark its state as failed.
        let Some(signal) = self.signal.take() else {
            return;
        };
        signal.set(MessageStatus::Failed);
    }
}

#[derive(Debug)]
struct SinkActor<M, S>
where
    M: From<RawMessage>,
    S: SinkExt<M, Error = WSError> + Unpin,
{
    receiver: async_channel::Receiver<InRawMessage>,
    abort_receiver: async_channel::Receiver<()>,
    sink: S,
    phantom: PhantomData<M>,
}

impl<M, S> SinkActor<M, S>
where
    M: From<RawMessage>,
    S: SinkExt<M, Error = WSError> + Unpin,
{
    async fn run(&mut self) -> Result<(), WSError> {
        loop {
            futures::select! {
                res = self.receiver.recv().fuse() => {
                    let Ok(mut inmessage) = res else {
                        break;
                    };
                    let Some(message) = inmessage.take_message() else {
                        continue;
                    };
                    tracing::trace!("sending message: {:?}", message);
                    match self.sink.send(M::from(message)).await {
                        Ok(()) => inmessage.set_signal(MessageStatus::Sent),
                        Err(err) => {
                            inmessage.set_signal(MessageStatus::Failed);
                            tracing::warn!(?err, "sink send failed");
                            return Err(err);
                        }
                    }
                },
                _ = &mut self.abort_receiver.recv().fuse() => {
                    break;
                },
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Sink {
    sender: async_channel::Sender<InRawMessage>,
}

impl Sink {
    fn new<M, S>(
        sink: S,
        abort_receiver: async_channel::Receiver<()>,
        handle: impl enfync::Handle,
    ) -> (enfync::PendingResult<Result<(), WSError>>, Self)
    where
        M: From<RawMessage> + Send + 'static,
        S: SinkExt<M, Error = WSError> + Unpin + Send + 'static,
    {
        let (sender, receiver) = async_channel::unbounded();
        let mut actor = SinkActor {
            receiver,
            abort_receiver,
            sink,
            phantom: Default::default(),
        };
        let future = handle.spawn(async move { actor.run().await });
        (future, Self { sender })
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    pub async fn send(
        &self,
        inmessage: InMessage,
    ) -> Result<(), async_channel::SendError<InRawMessage>> {
        self.sender.send(inmessage.into()).await
    }

    pub(crate) async fn send_raw(
        &self,
        inmessage: InRawMessage,
    ) -> Result<(), async_channel::SendError<InRawMessage>> {
        self.sender.send(inmessage).await
    }
}

#[derive(Debug)]
struct StreamActor<M, S>
where
    M: Into<RawMessage>,
    S: StreamExt<Item = Result<M, WSError>> + Unpin,
{
    sender: async_channel::Sender<Result<Message, WSError>>,
    stream: S,
    last_alive: Arc<Mutex<Instant>>,
}

impl<M, S> StreamActor<M, S>
where
    M: Into<RawMessage>,
    S: StreamExt<Item = Result<M, WSError>> + Unpin,
{
    async fn run(mut self) {
        while let Some(result) = self.stream.next().await {
            let result = result.map(M::into);
            tracing::trace!("received message: {:?}", result);
            *self.last_alive.lock().await = Instant::now();

            let mut closing = false;
            let message = match result {
                Ok(message) => Ok(match message {
                    RawMessage::Text(text) => Message::Text(text),
                    RawMessage::Binary(bytes) => Message::Binary(bytes),
                    RawMessage::Ping(bytes) => Message::Ping(bytes),
                    RawMessage::Pong(bytes) => {
                        if let Ok(bytes) = bytes.clone().try_into() {
                            let bytes: [u8; 16] = bytes;
                            let timestamp = u128::from_be_bytes(bytes);
                            let timestamp = Duration::from_millis(timestamp as u64); // TODO: handle overflow
                            let latency = SystemTime::now()
                                .duration_since(UNIX_EPOCH + timestamp)
                                .unwrap_or_default();
                            // TODO: handle time zone
                            tracing::trace!("latency: {}ms", latency.as_millis());
                        }

                        Message::Pong(bytes)
                    }
                    RawMessage::Close(frame) => {
                        closing = true;
                        Message::Close(frame)
                    }
                }),
                Err(err) => Err(err), // maybe early return here?
            };
            if self.sender.send(message).await.is_err() {
                // In websockets, you always echo a close frame received from your connection partner back to them.
                // This means a normal close sequence will always end with the following line emitted by the socket of
                // the client/server that initiated the close sequence (in response to the close frame echoed by their
                // partner).
                if closing {
                    tracing::trace!("stream is closed");
                } else {
                    tracing::warn!("failed to forward message, stream is disconnected");
                }
                break;
            };
        }
    }
}

#[derive(Debug)]
pub struct Stream {
    receiver: async_channel::Receiver<Result<Message, WSError>>,
}

impl Stream {
    fn new<M, S>(
        stream: S,
        last_alive: Arc<Mutex<Instant>>,
        handle: impl enfync::Handle,
    ) -> (enfync::PendingResult<()>, Self)
    where
        M: Into<RawMessage> + std::fmt::Debug + Send + 'static,
        S: StreamExt<Item = Result<M, WSError>> + Unpin + Send + 'static,
    {
        let (sender, receiver) = async_channel::unbounded();
        let actor = StreamActor {
            sender,
            stream,
            last_alive,
        };
        let future = handle.spawn(actor.run());

        (future, Self { receiver })
    }

    pub async fn recv(&mut self) -> Option<Result<Message, WSError>> {
        self.receiver.recv().await.ok()
    }
}

#[derive(Debug)]
pub struct Socket {
    pub sink: Sink,
    pub stream: Stream,
    sink_result_receiver: Option<async_channel::Receiver<Result<(), WSError>>>,
}

impl Socket {
    pub fn new<M, E, S>(socket: S, config: SocketConfig, handle: impl enfync::Handle) -> Self
    where
        M: Into<RawMessage> + From<RawMessage> + std::fmt::Debug + Send + 'static,
        E: Into<WSError> + std::error::Error,
        S: SinkExt<M, Error = E> + Unpin + StreamExt<Item = Result<M, E>> + Unpin + Send + 'static,
    {
        let last_alive = Instant::now();
        let last_alive = Arc::new(Mutex::new(last_alive));
        let (sink, stream) = socket.sink_err_into().err_into().split();
        let (sink_abort_sender, sink_abort_receiver) = async_channel::bounded(1usize);
        let ((mut sink_future, sink), (mut stream_future, stream)) = (
            Sink::new(sink, sink_abort_receiver, handle.clone()),
            Stream::new(stream, last_alive.clone(), handle.clone()),
        );
        let (hearbeat_abort_sender, hearbeat_abort_receiver) = async_channel::bounded(1usize);
        let sink_clone = sink.clone();
        handle.spawn(async move {
            socket_heartbeat(sink_clone, config, hearbeat_abort_receiver, last_alive).await
        });

        let (sink_result_sender, sink_result_receiver) = async_channel::bounded(1usize);
        handle.spawn(async move {
            let _ = stream_future.extract().await;
            let _ = sink_abort_sender.send_blocking(());
            let _ = hearbeat_abort_sender.send_blocking(());
            let _ = sink_result_sender.send_blocking(
                sink_future
                    .extract()
                    .await
                    .unwrap_or(Err(WSError::AlreadyClosed)),
            );
        });

        Self {
            sink,
            stream,
            sink_result_receiver: Some(sink_result_receiver),
        }
    }

    pub async fn send(
        &self,
        message: InMessage,
    ) -> Result<(), async_channel::SendError<InRawMessage>> {
        self.sink.send(message).await
    }

    pub async fn send_raw(
        &self,
        message: InRawMessage,
    ) -> Result<(), async_channel::SendError<InRawMessage>> {
        self.sink.send_raw(message).await
    }

    pub async fn recv(&mut self) -> Option<Result<Message, WSError>> {
        self.stream.recv().await
    }

    pub(crate) async fn await_sink_close(&mut self) -> Result<(), WSError> {
        let Some(sink_result_receiver) = self.sink_result_receiver.take() else {
            return Err(WSError::AlreadyClosed);
        };
        sink_result_receiver
            .recv()
            .await
            .unwrap_or(Err(WSError::AlreadyClosed))
    }
}

#[cfg(not(target_family = "wasm"))]
async fn socket_heartbeat(
    sink: Sink,
    config: SocketConfig,
    abort_receiver: async_channel::Receiver<()>,
    last_alive: Arc<Mutex<Instant>>,
) {
    let sleep = tokio::time::sleep(config.heartbeat);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep => {
                let Some(next_sleep_duration) = handle_heartbeat_sleep_elapsed(&sink, &config, &last_alive).await else {
                    break;
                };
                sleep.as_mut().reset(tokio::time::Instant::now() + next_sleep_duration);
            }
            _ = abort_receiver.recv() => break,
        }
    }
}

#[cfg(target_family = "wasm")]
async fn socket_heartbeat(
    sink: Sink,
    config: SocketConfig,
    abort_receiver: async_channel::Receiver<()>,
    last_alive: Arc<Mutex<Instant>>,
) {
    let mut sleep_duration = config.heartbeat;

    loop {
        // It is better to use Sleep::reset(), but we can't do it here because fuse() consumes the sleep
        // and we need futures::select since we can't use tokio on WASM targets.
        let sleep = wasmtimer::tokio::sleep(sleep_duration).fuse();
        futures::pin_mut!(sleep);
        futures::select! {
            _ = sleep => {
                let Some(next_sleep_duration) = handle_heartbeat_sleep_elapsed(&sink, &config, &last_alive).await else {
                    break;
                };
                sleep_duration = next_sleep_duration;
            }
            _ = &mut abort_receiver.recv().fuse() => break,
        }
    }
}

async fn handle_heartbeat_sleep_elapsed(
    sink: &Sink,
    config: &SocketConfig,
    last_alive: &Arc<Mutex<Instant>>,
) -> Option<Duration> {
    // check last alive
    let elapsed_since_last_alive = last_alive.lock().await.elapsed();
    if elapsed_since_last_alive > config.timeout {
        tracing::info!("closing connection due to timeout");
        let _ = sink
            .send_raw(InRawMessage::new(RawMessage::Close(Some(CloseFrame {
                code: CloseCode::Abnormal,
                reason: String::from("remote partner is inactive"),
            }))))
            .await;
        return None;
    } else if elapsed_since_last_alive < config.heartbeat {
        // todo: this branch will needlessly fire at least once per heartbeat for idle connections since
        //       Pongs arrive after some delay
        return Some(config.heartbeat.saturating_sub(elapsed_since_last_alive));
    }

    // send ping
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    if sink
        .send_raw(InRawMessage::new((config.heartbeat_ping_msg_fn)(timestamp)))
        .await
        .is_err()
    {
        return None;
    }

    Some(config.heartbeat)
}
