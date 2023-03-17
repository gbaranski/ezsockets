# ezsockets

Creating a WebSocket server or a client in Rust can be troublesome. This crate facilitates this process by providing:

- Traits to allow declarative and event-based programming.
- Heartbeat mechanism to keep the connection alive.
- Automatic reconnection of WebSocket Client.
- Support for multiple back-ends such as Axum or Tungstenite.
- TLS support for servers with `rustls` and `native-tls`.

## Documentation

View the full documentation at [docs.rs/ezsockets](http://docs.rs/ezsockets)

## Examples
- [`simple-client`](/examples/simple-client) - a simplest WebSocket client which uses stdin as input.
- [`echo-server`](/examples/echo-server) - server that echoes back every message it receives.
- [`counter-server`](/examples/counter-server) - server that increments global value every second and shares it with client
- [`chat-client`](/examples/chat-client) - chat client for `chat-server` and `chat-server-axum` examples
- [`chat-server`](/examples/chat-server) - chat server with support of rooms
- [`chat-server-axum`](/examples/chat-server-axum) - same as above, but using `axum` as a back-end


## Client

[`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite) is being used under the hood.

See [examples/simple-client](/examples/simple-client) for a simple usage
and [docs.rs/ezsockets/server](https://docs.rs/ezsockets/latest/ezsockets/client/index.html) for documentation.

## Server

WebSocket server can use one of supported back-ends:
- [`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite) - the simplest way to get started.
- [`axum`](https://github.com/tokio-rs/axum) - ergonomic and modular web framework built with Tokio, Tower, and Hyper
- [`actix-web`](https://github.com/actix/actix-web) - Work in progress at [#22](https://github.com/gbaranski/ezsockets/issues/22)

See [examples/echo-server](/examples/echo-server) for a simple usage
and [docs.rs/ezsockets/server](https://docs.rs/ezsockets/latest/ezsockets/server/index.html) for documentation.
