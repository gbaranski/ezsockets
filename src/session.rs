use std::fmt::Formatter;
use std::sync::Arc;

use crate::socket::{InMessage, MessageSignal};
use crate::CloseFrame;
use crate::Error;
use crate::Message;
use crate::Socket;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

#[async_trait]
pub trait SessionExt: Send {
    /// Custom identification number of SessionExt, usually a number or a string.
    type ID: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display;
    /// Type the custom call - parameters passed to `on_call`.
    type Call: Send;

    /// Returns ID of the session.
    fn id(&self) -> &Self::ID;
    /// Handler for text messages from the client. Returning an error will force-close the session.
    async fn on_text(&mut self, text: String) -> Result<(), Error>;
    /// Handler for binary messages from the client. Returning an error will force-close the session.
    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
    /// Handler for custom calls from other parts from your program. Returning an error will force-close the session.
    /// This is useful for concurrency and polymorphism.
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error>;
}

type CloseReceiver = oneshot::Receiver<Result<Option<CloseFrame>, Error>>;

pub struct Session<I, C> {
    pub id: I,
    to_socket_sender: mpsc::UnboundedSender<InMessage>,
    session_call_sender: mpsc::UnboundedSender<C>,
    closed_indicator: Arc<Mutex<Option<CloseReceiver>>>,
}

impl<I: std::fmt::Debug, C> std::fmt::Debug for Session<I, C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<I: Clone, C> Clone for Session<I, C> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            to_socket_sender: self.to_socket_sender.clone(),
            session_call_sender: self.session_call_sender.clone(),
            closed_indicator: self.closed_indicator.clone(),
        }
    }
}

impl<I: std::fmt::Display + Clone + Send, C: Send> Session<I, C> {
    pub fn create<S: SessionExt<ID = I, Call = C> + 'static>(
        session_fn: impl FnOnce(Session<I, C>) -> S,
        session_id: I,
        socket: Socket,
    ) -> Self {
        let (to_socket_sender, to_socket_receiver) = mpsc::unbounded_channel();
        let (session_call_sender, session_call_receiver) = mpsc::unbounded_channel();
        let (closed_sender, closed_receiver) = oneshot::channel();
        let handle = Self {
            id: session_id.clone(),
            to_socket_sender,
            session_call_sender,
            closed_indicator: Arc::new(Mutex::new(Some(closed_receiver))),
        };
        let session = session_fn(handle.clone());
        let mut actor = SessionActor::new(
            session,
            session_id,
            to_socket_receiver,
            session_call_receiver,
            socket,
        );

        tokio::spawn(async move {
            let result = actor.run().await;
            closed_sender.send(result).unwrap_or_default();
        });

        handle
    }
}

impl<I: std::fmt::Display + Clone, C> Session<I, C> {
    #[doc(hidden)]
    /// WARN: Use only if really nessesary.
    ///
    /// This uses some hack, which takes ownership of underlying `oneshot::Receiver`, making it inaccessible
    /// for all future calls of this method.
    pub(super) async fn await_close(&self) -> Result<Option<CloseFrame>, Error> {
        let mut closed_indicator = self.closed_indicator.lock().await;
        let closed_indicator = closed_indicator
            .take()
            .expect("someone already called .await_close() before");
        closed_indicator
            .await
            .unwrap_or(Err("session actor crashed".into()))
    }

    /// Checks if the Session is still alive, if so you can proceed sending calls or messages.
    pub fn alive(&self) -> bool {
        !self.to_socket_sender.is_closed() && !self.session_call_sender.is_closed()
    }

    /// Sends a Text message to the server
    pub fn text(
        &self,
        text: impl Into<String>,
    ) -> Result<MessageSignal, mpsc::error::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Text(text.into()));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send(inmessage)
            .map(|_| inmessage_signal)
    }

    /// Sends a Binary message to the server
    pub fn binary(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<MessageSignal, mpsc::error::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Binary(bytes.into()));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send(inmessage)
            .map(|_| inmessage_signal)
    }

    /// Calls a method on the session
    pub fn call(&self, call: C) -> Result<(), mpsc::error::SendError<C>> {
        self.session_call_sender.send(call)
    }

    /// Calls a method on the session, allowing the Session to respond with oneshot::Sender.
    /// This is just for easier construction of the call which happen to contain oneshot::Sender in it.
    pub async fn call_with<R: std::fmt::Debug>(
        &self,
        f: impl FnOnce(oneshot::Sender<R>) -> C,
    ) -> Option<R> {
        let (sender, receiver) = oneshot::channel();
        let call = f(sender);

        let Ok(_) = self.session_call_sender.send(call) else {
            return None;
        };
        let Ok(result) = receiver.await else {
            return None;
        };
        Some(result)
    }

    /// Close the session. Returns an error if the session is already closed.
    pub fn close(
        &self,
        frame: Option<CloseFrame>,
    ) -> Result<MessageSignal, mpsc::error::SendError<InMessage>> {
        let inmessage = InMessage::new(Message::Close(frame));
        let inmessage_signal = inmessage.clone_signal().unwrap(); //safety: always available on construction
        self.to_socket_sender
            .send(inmessage)
            .map(|_| inmessage_signal)
    }
}

pub(crate) struct SessionActor<E: SessionExt> {
    pub extension: E,
    id: E::ID,
    to_socket_receiver: mpsc::UnboundedReceiver<InMessage>,
    session_call_receiver: mpsc::UnboundedReceiver<E::Call>,
    socket: Socket,
}

impl<E: SessionExt> SessionActor<E> {
    pub(crate) fn new(
        extension: E,
        id: E::ID,
        to_socket_receiver: mpsc::UnboundedReceiver<InMessage>,
        session_call_receiver: mpsc::UnboundedReceiver<E::Call>,
        socket: Socket,
    ) -> Self {
        Self {
            extension,
            id,
            to_socket_receiver,
            session_call_receiver,
            socket,
        }
    }

    pub(crate) async fn run(&mut self) -> Result<Option<CloseFrame>, Error> {
        loop {
            tokio::select! {
                biased;
                Some(inmessage) = self.to_socket_receiver.recv() => {
                    let mut close_frame = match &inmessage.message {
                        Some(Message::Close(frame)) => Some(frame.clone()),
                        _ => None,
                    };
                    if self.socket.send(inmessage).await.is_err() && close_frame.is_none() {
                        close_frame = Some(None);
                    }
                    if let Some(frame) = close_frame {
                        // session closed itself
                        return Ok(frame)
                    }
                }
                Some(call) = self.session_call_receiver.recv() => {
                    self.extension.on_call(call).await?;
                }
                message = self.socket.recv() => {
                    match message {
                        Some(Ok(message)) => match message {
                            Message::Text(text) => self.extension.on_text(text).await?,
                            Message::Binary(bytes) => self.extension.on_binary(bytes).await?,
                            Message::Close(frame) => {
                                // closed by client
                                return Ok(frame.map(CloseFrame::from))
                            },
                        }
                        Some(Err(error)) => {
                            tracing::warn!(id = %self.id, "connection error: {error}");
                        }
                        None => break
                    };
                }
                else => break,
            }
        }
        Ok(None)
    }
}
