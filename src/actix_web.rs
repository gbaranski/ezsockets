// This code comes mostly from https://github.com/actix/actix-web and actix-web-actors crate

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use actix_http::ws::hash_key;
pub use actix_http::ws::{CloseCode, CloseReason, Frame, HandshakeError, Message, ProtocolError};
use actix_web::{
    error::Error,
    http::{
        header::{self, HeaderValue},
        Method, StatusCode,
    },
    web, HttpRequest, HttpResponse,
};
use actix_web_crate as actix_web;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite;

use crate::{socket::Config, Server, ServerExt, SessionExt, Socket};

pub async fn accept<SE, SX>(
    req: HttpRequest,
    payload: web::Payload,
    server: Server<SE>,
    args: <SE::Session as SessionExt>::Args,
) -> Result<(HttpResponse, SX::ID), Error> 
where 
    SE: ServerExt<Session = SX>,
    SX: SessionExt,
{
    // WebSocket accepts only GET
    if *req.method() != Method::GET {
        Err(HandshakeError::GetMethodRequired)?;
    }

    // check for "UPGRADE" to WebSocket header
    let has_hdr = if let Some(hdr) = req.headers().get(&header::UPGRADE) {
        if let Ok(s) = hdr.to_str() {
            s.to_ascii_lowercase().contains("websocket")
        } else {
            false
        }
    } else {
        false
    };
    if !has_hdr {
        Err(HandshakeError::NoWebsocketUpgrade)?
    }

    // Upgrade connection
    if !req.head().upgrade() {
        Err(HandshakeError::NoConnectionUpgrade)?
    }

    // check supported version
    if !req.headers().contains_key(&header::SEC_WEBSOCKET_VERSION) {
        Err(HandshakeError::NoVersionHeader)?
    }
    let supported_ver = {
        if let Some(hdr) = req.headers().get(&header::SEC_WEBSOCKET_VERSION) {
            hdr == "13" || hdr == "8" || hdr == "7"
        } else {
            false
        }
    };
    if !supported_ver {
        Err(HandshakeError::UnsupportedVersion)?
    }

    // check client handshake for validity
    if !req.headers().contains_key(&header::SEC_WEBSOCKET_KEY) {
        Err(HandshakeError::BadWebsocketKey)?
    }
    let key = {
        let key = req.headers().get(&header::SEC_WEBSOCKET_KEY).unwrap();
        hash_key(key.as_ref())
    };

    // TODO: Remove this
    let protocols: &[&'static str] = &[];
    // check requested protocols
    let protocol = req
        .headers()
        .get(&header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|req_protocols| {
            let req_protocols = req_protocols.to_str().ok()?;
            req_protocols
                .split(',')
                .map(|req_p| req_p.trim())
                .find(|req_p| protocols.iter().any(|p| p == req_p))
        });

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

    // TODO: Somehow construct a stream that satisfies AsyncRead + AsyncWrite + Unpin
    let stream = (|| todo!())();
    // The TcpStream is just for now, to satisfy the trait bounds
    let websocket_stream = tokio_tungstenite::WebSocketStream::<TcpStream>::from_raw_socket(
        stream,
        tungstenite::protocol::Role::Server,
        None,
    )
    .await;

    let socket = Socket::new(websocket_stream, Config::default());

    let address = req
        .peer_addr()
        .or_else(|| {
            // Using this random address, because the `peer_addr()` is going to return `None` only during the unit test anyways
            Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(123, 123, 123, 123),
                1234,
            )))
        })
        .unwrap();

    let session_id = server.accept(socket, address, args).await;

    let response = response.await?;
    Ok((response, session_id))
}
