# ezsockets

Creating a WebSocket server or a client in Rust can be troublesome. This crate facilitates this process by providing:

- Traits to allow declarative and event-based programming.
- Easy concurrency with Tokio and async/await. Server sessions are Clone'able and can be shared between tasks.
- Heartbeat mechanism to keep the connection alive.
- Automatic reconnection of WebSocket Clients.
- Support for arbitrary client back-ends, with built-in native and WASM client connectors.
- Support for multiple server back-ends such as Axum or Tungstenite.
- TLS support for servers with `rustls` and `native-tls`.

## Documentation

View the full documentation at [docs.rs/ezsockets](http://docs.rs/ezsockets)

## Examples
- [`simple-client`](https://github.com/gbaranski/ezsockets/tree/master/examples/simple-client) - a simplest WebSocket client which uses stdin as input.
- [`echo-server`](https://github.com/gbaranski/ezsockets/tree/master/examples/echo-server) - server that echoes back every message it receives.
- [`echo-server`](https://github.com/gbaranski/ezsockets/tree/master/examples/echo-server-native-tls) - same as `echo-server`, but with `native-tls`.
- [`counter-server`](https://github.com/gbaranski/ezsockets/tree/master/examples/counter-server) - server that increments global value every second and shares it with client
- [`chat-client`](https://github.com/gbaranski/ezsockets/tree/master/examples/chat-client) - chat client for `chat-server` and `chat-server-axum` examples
- [`wasm-client`](https://github.com/gbaranski/ezsockets/tree/master/examples/chat-client-wasm) - chat client for `chat-server` and `chat-server-axum` examples that runs in the browser (only listens to the chat)
- [`chat-server`](https://github.com/gbaranski/ezsockets/tree/master/examples/chat-server) - chat server with support of rooms
- [`chat-server-axum`](https://github.com/gbaranski/ezsockets/tree/master/examples/chat-server-axum) - same as above, but using `axum` as a back-end


## Client

By default clients use [`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite) under the hood. Disable default features and enable `wasm_client` to run clients on WASM targets.

See [examples/simple-client](https://github.com/gbaranski/ezsockets/tree/master/examples/simple-client) for a simple usage
and [docs.rs/ezsockets/server](https://docs.rs/ezsockets/latest/ezsockets/client/index.html) for documentation.

## Server

WebSocket server can use one of the supported back-ends:
- [`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite) - the simplest way to get started.
- [`axum`](https://github.com/tokio-rs/axum) - ergonomic and modular web framework built with Tokio, Tower, and Hyper
- [`actix-web`](https://github.com/actix/actix-web) - Work in progress at [#22](https://github.com/gbaranski/ezsockets/issues/22)

See [examples/echo-server](https://github.com/gbaranski/ezsockets/tree/master/examples/echo-server) for a simple usage
and [docs.rs/ezsockets/server](https://docs.rs/ezsockets/latest/ezsockets/server/index.html) for documentation.

# License

Licensed under [MIT](https://choosealicense.com/licenses/mit/).

# Contact

Reach me out on Discord `gbaranski#5119`, or mail me at me@gbaranski.com.
