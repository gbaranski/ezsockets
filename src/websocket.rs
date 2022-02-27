// Significant part of this code is licensed by the MIT License by tokio-tungstenite authors.
// https://github.com/snapview/tokio-tungstenite

// Copyright (c) 2017 Daniel Abramov
// Copyright (c) 2017 Alexey Galakhov

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.


#[derive(Debug)]
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

use tokio_tungstenite::tungstenite;
use tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;

impl From<CloseCode> for TungsteniteCloseCode {
    fn from(code: CloseCode) -> Self {
        match code {
            CloseCode::Normal => Self::Normal,
            CloseCode::Away => Self::Away,
            CloseCode::Protocol => Self::Protocol,
            CloseCode::Unsupported => Self::Unsupported,
            CloseCode::Status => Self::Status,
            CloseCode::Abnormal => Self::Abnormal,
            CloseCode::Invalid => Self::Invalid,
            CloseCode::Policy => Self::Policy,
            CloseCode::Size => Self::Size,
            CloseCode::Extension => Self::Extension,
            CloseCode::Error => Self::Error,
            CloseCode::Restart => Self::Restart,
            CloseCode::Again => Self::Again,
            CloseCode::Tls => Self::Tls,
            CloseCode::Reserved(v) => Self::Reserved(v),
            CloseCode::Iana(v) => Self::Iana(v),
            CloseCode::Library(v) => Self::Library(v),
            CloseCode::Bad(v) => Self::Bad(v),
        }
    }
}

impl From<TungsteniteCloseCode> for CloseCode {
    fn from(code: TungsteniteCloseCode) -> Self {
        match code {
            TungsteniteCloseCode::Normal => Self::Normal,
            TungsteniteCloseCode::Away => Self::Away,
            TungsteniteCloseCode::Protocol => Self::Protocol,
            TungsteniteCloseCode::Unsupported => Self::Unsupported,
            TungsteniteCloseCode::Status => Self::Status,
            TungsteniteCloseCode::Abnormal => Self::Abnormal,
            TungsteniteCloseCode::Invalid => Self::Invalid,
            TungsteniteCloseCode::Policy => Self::Policy,
            TungsteniteCloseCode::Size => Self::Size,
            TungsteniteCloseCode::Extension => Self::Extension,
            TungsteniteCloseCode::Error => Self::Error,
            TungsteniteCloseCode::Restart => Self::Restart,
            TungsteniteCloseCode::Again => Self::Again,
            TungsteniteCloseCode::Tls => Self::Tls,
            TungsteniteCloseCode::Reserved(v) => Self::Reserved(v),
            TungsteniteCloseCode::Iana(v) => Self::Iana(v),
            TungsteniteCloseCode::Library(v) => Self::Library(v),
            TungsteniteCloseCode::Bad(v) => Self::Bad(v),
        }
    }
}


#[derive(Debug)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<(CloseCode, String)>),
}

impl From<Message> for tungstenite::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => tungstenite::Message::Text(text),
            Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
            Message::Close(frame) => tungstenite::Message::Close(frame.map(|(code, reason)| {
                tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                }
            })),
        }
    }
}