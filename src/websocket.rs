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

use crate::socket::RawMessage;
use tokio_tungstenite::tungstenite;
use tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;
use crate::CloseCode;
use crate::CloseFrame;


impl<'t> From<tungstenite::protocol::CloseFrame<'t>> for CloseFrame {
    fn from(frame: tungstenite::protocol::CloseFrame) -> Self {
        Self {
            code: frame.code.into(),
            reason: frame.reason.into(),
        }
    }
}

impl<'t> From<CloseFrame> for tungstenite::protocol::CloseFrame<'t> {
    fn from(frame: CloseFrame) -> Self {
        Self {
            code: frame.code.into(),
            reason: frame.reason.into(),
        }
    }
}

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

impl From<tungstenite::Message> for RawMessage {
    fn from(message: tungstenite::Message) -> Self {
        match message {
            tungstenite::Message::Text(text) => Self::Text(text),
            tungstenite::Message::Binary(bytes) => Self::Binary(bytes),
            tungstenite::Message::Ping(bytes) => Self::Ping(bytes),
            tungstenite::Message::Pong(bytes) => Self::Pong(bytes),
            tungstenite::Message::Close(frame) => Self::Close(frame.map(CloseFrame::from)),
            tungstenite::Message::Frame(_) => unreachable!(),
        }
    }
}

impl From<RawMessage> for tungstenite::Message {
    fn from(message: RawMessage) -> Self {
        match message {
            RawMessage::Text(text) => Self::Text(text),
            RawMessage::Binary(bytes) => Self::Binary(bytes),
            RawMessage::Ping(bytes) => Self::Ping(bytes),
            RawMessage::Pong(bytes) => Self::Pong(bytes),
            RawMessage::Close(frame) => Self::Close(frame.map(CloseFrame::into)),
        }
    }
}
