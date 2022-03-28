use std::collections::VecDeque;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use crate::Server;
use crate::ServerExt;
use actix_http::header;
use actix_http::ws::Codec;
use actix_http::ws::hash_key;
pub use actix_http::ws::{CloseCode, CloseReason, Frame, HandshakeError, Message, ProtocolError};
use actix_http::StatusCode;
use actix_web::error::Error;
use actix_web::error::PayloadError;
use actix_web::web::Bytes;
use actix_web::HttpRequest;
use actix_web::HttpResponse;
use actix_web::HttpResponseBuilder;
use actix_web::web::BytesMut;
use futures::Stream;
use http::HeaderValue;

pub fn start<E, T>(server: Server<E>, req: &HttpRequest, stream: T) -> Result<HttpResponse, Error>
where
    E: ServerExt,
    T: Stream<Item = Result<Bytes, PayloadError>> + Send + Unpin + 'static,
{
    let mut res = handshake(req)?;
    Ok(res.streaming(WebsocketContext::create(server, stream)))
}

pub fn handshake(req: &HttpRequest) -> Result<HttpResponseBuilder, HandshakeError> {
    // WebSocket accepts only GET
    // check supported version
    if !req.headers().contains_key(&header::SEC_WEBSOCKET_VERSION) {
        return Err(HandshakeError::NoVersionHeader);
    }
    let supported_ver = {
        if let Some(hdr) = req.headers().get(&header::SEC_WEBSOCKET_VERSION) {
            hdr == "13" || hdr == "8" || hdr == "7"
        } else {
            false
        }
    };
    if !supported_ver {
        return Err(HandshakeError::UnsupportedVersion);
    }

    // check client handshake for validity
    if !req.headers().contains_key(&header::SEC_WEBSOCKET_KEY) {
        return Err(HandshakeError::BadWebsocketKey);
    }
    let key = {
        let key = req.headers().get(&header::SEC_WEBSOCKET_KEY).unwrap();
        hash_key(key.as_ref())
    };

    // check requested protocols
    let protocol = req.headers().get(&header::SEC_WEBSOCKET_PROTOCOL);

    let mut response = HttpResponse::build(StatusCode::SWITCHING_PROTOCOLS)
        .upgrade("websocket")
        .insert_header((
            header::SEC_WEBSOCKET_ACCEPT,
            // key is known to be header value safe ascii
            HeaderValue::from_bytes(&key).unwrap(),
        ))
        .take();

    if let Some(protocol) = protocol {
        response.insert_header((header::SEC_WEBSOCKET_PROTOCOL, protocol));
    }

    Ok(response)
}

/// Execution context for `WebSockets` actors
pub struct WebsocketContext<E: ServerExt> {
    inner: Server<E>,
    messages: VecDeque<Option<Message>>,
}

impl<E: ServerExt> WebsocketContext<E> {
    /// Create a new Websocket context from a request and an actor.
    #[inline]
    pub fn create<S>(server: Server<E>, stream: S) -> impl Stream<Item = Result<Bytes, Error>>
    where
        S: Stream<Item = Result<Bytes, PayloadError>> + Send + Unpin + 'static,
    {
        let ctx = WebsocketContext {
            inner: server.clone(),
            messages: VecDeque::new(),
        };
        WebsocketContextFut::new(ctx, server, Codec::new())
    }
}

struct WebsocketContextFut<E: ServerExt>
{
    server: Server<E>,
    encoder: Codec,
    buf: BytesMut,
    closed: bool,
}

impl<E: ServerExt> WebsocketContextFut<E>
{
    fn new(ctx: WebsocketContext<E>, server: Server<E>, codec: Codec) -> Self {
        Self {
            server,
            encoder: codec,
            buf: BytesMut::new(),
            closed: false,
        }
    }
}

impl<E: ServerExt> Stream for WebsocketContextFut<E>
{
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.fut.alive() {
            let _ = Pin::new(&mut this.fut).poll(cx);
        }

        // encode messages
        while let Some(item) = this.fut.ctx().messages.pop_front() {
            if let Some(msg) = item {
                this.encoder.encode(msg, &mut this.buf)?;
            } else {
                this.closed = true;
                break;
            }
        }

        if !this.buf.is_empty() {
            Poll::Ready(Some(Ok(this.buf.split().freeze())))
        } else if this.fut.alive() && !this.closed {
            Poll::Pending
        } else {
            Poll::Ready(None)
        }
    }
}

// impl<A, M> ToEnvelope<A, M> for WebsocketContext<A>
// where
//     A: Actor<Context = WebsocketContext<A>> + Handler<M>,
//     M: ActixMessage + Send + 'static,
//     M::Result: Send,
// {
//     fn pack(msg: M, tx: Option<oneshot::Sender<M::Result>>) -> Envelope<A> {
//         Envelope::new(msg, tx)
//     }
// }

// pin_project! {
//     #[derive(Debug)]
//     struct WsStream<S> {
//         #[pin]
//         stream: S,
//         decoder: Codec,
//         buf: BytesMut,
//         closed: bool,
//     }
// }

// impl<S> WsStream<S>
// where
//     S: Stream<Item = Result<Bytes, PayloadError>>,
// {
//     fn new(stream: S, codec: Codec) -> Self {
//         Self {
//             stream,
//             decoder: codec,
//             buf: BytesMut::new(),
//             closed: false,
//         }
//     }
// }

// impl<S> Stream for WsStream<S>
// where
//     S: Stream<Item = Result<Bytes, PayloadError>>,
// {
//     type Item = Result<Message, ProtocolError>;

//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         let mut this = self.as_mut().project();

//         if !*this.closed {
//             loop {
//                 match Pin::new(&mut this.stream).poll_next(cx) {
//                     Poll::Ready(Some(Ok(chunk))) => {
//                         this.buf.extend_from_slice(&chunk[..]);
//                     }
//                     Poll::Ready(None) => {
//                         *this.closed = true;
//                         break;
//                     }
//                     Poll::Pending => break,
//                     Poll::Ready(Some(Err(e))) => {
//                         return Poll::Ready(Some(Err(ProtocolError::Io(io::Error::new(
//                             io::ErrorKind::Other,
//                             format!("{}", e),
//                         )))));
//                     }
//                 }
//             }
//         }

//         match this.decoder.decode(this.buf)? {
//             None => {
//                 if *this.closed {
//                     Poll::Ready(None)
//                 } else {
//                     Poll::Pending
//                 }
//             }
//             Some(frm) => {
//                 let msg = match frm {
//                     Frame::Text(data) => {
//                         Message::Text(ByteString::try_from(data).map_err(|e| {
//                             ProtocolError::Io(io::Error::new(
//                                 io::ErrorKind::Other,
//                                 format!("{}", e),
//                             ))
//                         })?)
//                     }
//                     Frame::Binary(data) => Message::Binary(data),
//                     Frame::Ping(s) => Message::Ping(s),
//                     Frame::Pong(s) => Message::Pong(s),
//                     Frame::Close(reason) => Message::Close(reason),
//                     Frame::Continuation(item) => Message::Continuation(item),
//                 };
//                 Poll::Ready(Some(Ok(msg)))
//             }
//         }
//     }
// }

// #[cfg(test)]
// mod tests {
//     use actix_web::{
//         http::{header, Method},
//         test::TestRequest,
//     };

//     use super::*;

//     #[test]
//     fn test_handshake() {
//         let req = TestRequest::default()
//             .method(Method::POST)
//             .to_http_request();
//         assert_eq!(
//             HandshakeError::GetMethodRequired,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default().to_http_request();
//         assert_eq!(
//             HandshakeError::NoWebsocketUpgrade,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default()
//             .insert_header((header::UPGRADE, header::HeaderValue::from_static("test")))
//             .to_http_request();
//         assert_eq!(
//             HandshakeError::NoWebsocketUpgrade,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .to_http_request();
//         assert_eq!(
//             HandshakeError::NoConnectionUpgrade,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .to_http_request();
//         assert_eq!(
//             HandshakeError::NoVersionHeader,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_VERSION,
//                 header::HeaderValue::from_static("5"),
//             ))
//             .to_http_request();
//         assert_eq!(
//             HandshakeError::UnsupportedVersion,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_VERSION,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .to_http_request();
//         assert_eq!(
//             HandshakeError::BadWebsocketKey,
//             handshake(&req).err().unwrap()
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_VERSION,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_KEY,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .to_http_request();

//         let resp = handshake(&req).unwrap().finish();
//         assert_eq!(StatusCode::SWITCHING_PROTOCOLS, resp.status());
//         assert_eq!(None, resp.headers().get(&header::CONTENT_LENGTH));
//         assert_eq!(None, resp.headers().get(&header::TRANSFER_ENCODING));

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_VERSION,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_KEY,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_PROTOCOL,
//                 header::HeaderValue::from_static("graphql"),
//             ))
//             .to_http_request();

//         let protocols = ["graphql"];

//         assert_eq!(
//             StatusCode::SWITCHING_PROTOCOLS,
//             handshake_with_protocols(&req, &protocols)
//                 .unwrap()
//                 .finish()
//                 .status()
//         );
//         assert_eq!(
//             Some(&header::HeaderValue::from_static("graphql")),
//             handshake_with_protocols(&req, &protocols)
//                 .unwrap()
//                 .finish()
//                 .headers()
//                 .get(&header::SEC_WEBSOCKET_PROTOCOL)
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_VERSION,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_KEY,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_PROTOCOL,
//                 header::HeaderValue::from_static("p1, p2, p3"),
//             ))
//             .to_http_request();

//         let protocols = vec!["p3", "p2"];

//         assert_eq!(
//             StatusCode::SWITCHING_PROTOCOLS,
//             handshake_with_protocols(&req, &protocols)
//                 .unwrap()
//                 .finish()
//                 .status()
//         );
//         assert_eq!(
//             Some(&header::HeaderValue::from_static("p2")),
//             handshake_with_protocols(&req, &protocols)
//                 .unwrap()
//                 .finish()
//                 .headers()
//                 .get(&header::SEC_WEBSOCKET_PROTOCOL)
//         );

//         let req = TestRequest::default()
//             .insert_header((
//                 header::UPGRADE,
//                 header::HeaderValue::from_static("websocket"),
//             ))
//             .insert_header((
//                 header::CONNECTION,
//                 header::HeaderValue::from_static("upgrade"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_VERSION,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_KEY,
//                 header::HeaderValue::from_static("13"),
//             ))
//             .insert_header((
//                 header::SEC_WEBSOCKET_PROTOCOL,
//                 header::HeaderValue::from_static("p1,p2,p3"),
//             ))
//             .to_http_request();

//         let protocols = vec!["p3", "p2"];

//         assert_eq!(
//             StatusCode::SWITCHING_PROTOCOLS,
//             handshake_with_protocols(&req, &protocols)
//                 .unwrap()
//                 .finish()
//                 .status()
//         );
//         assert_eq!(
//             Some(&header::HeaderValue::from_static("p2")),
//             handshake_with_protocols(&req, &protocols)
//                 .unwrap()
//                 .finish()
//                 .headers()
//                 .get(&header::SEC_WEBSOCKET_PROTOCOL)
//         );
//     }
// }
