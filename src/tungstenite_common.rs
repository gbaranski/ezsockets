use crate::socket::RawMessage;
use crate::CloseCode;
use crate::CloseFrame;
use crate::Message;
use tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;

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
            CloseCode::Reserved(code) => Self::Reserved(code),
            CloseCode::Iana(code) => Self::Iana(code),
            CloseCode::Library(code) => Self::Library(code),
            CloseCode::Bad(code) => Self::Bad(code),
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
            TungsteniteCloseCode::Reserved(code) => Self::Reserved(code),
            TungsteniteCloseCode::Iana(code) => Self::Iana(code),
            TungsteniteCloseCode::Library(code) => Self::Library(code),
            TungsteniteCloseCode::Bad(code) => Self::Bad(code),
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

impl From<Message> for tungstenite::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => tungstenite::Message::Text(text),
            Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
            Message::Close(frame) => tungstenite::Message::Close(frame.map(CloseFrame::into)),
        }
    }
}
