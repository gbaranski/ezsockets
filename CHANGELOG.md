# Change Log
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

## v0.6.1

- Tighten cleanup guarantees for outgoing client messages in reconnect cycles.


## v0.6.0 BREAKING

- change `Client::close()` to use reference instead of `self`; remove `async` qualifier
- feat: loosen `std::fmt::Debug` constrain on `Call` by @qiujiangkun in https://github.com/gbaranski/ezsockets/pull/39
- refactor: replace `SessionExt::Args` with `http::Request` by @qiujiangkun and @gbaranski in https://github.com/gbaranski/ezsockets/pull/42
- add `ClientExt::on_disconnect()` for socket closure and return a `ClientCloseCode` from `ClientExt::on_close()/on_disconnect()` to control reconnect/full-close behavior
- add `Session::close()` method
- fix server bug that would cause the server to crash if `ServerExt::on_connect()` returned an error
- return `Err(Option<CloseFrame>)` from `ServerExt::on_connect()` to reject connections
- robustness: `Server`, `Client`, `Session`, `Sink` interfaces now return `Result<(), tokio::sync::mpsc::error::SendError>` instead of potentially panicking
- add reason to `ServerExt::on_disconnect()`
- improved tracing emitted during close sequences
- add `ClientConfig::query_parameter()` so connection requests can pass data via the URI (since additional connection headers are not supported by the websockets spec, this method should be more compatible with other implementations)
- removed panics from the internals
- downgraded tracing errors to warnings
- Return `Ok(MessageSignal)` from `Client` and `Session` `.binary()/.text()/.close()` endpoints instead of `Ok(())`. The `MessageSignal::state()` method will indicate the current state of the message (sending/sent/failed).
- Clients attempt to reconnect immediately instead of after one full reconnect interval.
- Incoming user messages are discarded while a client is reconnecting, to better match the usual behavior of a websocket connection. If you want messages to be buffered while reconnecting, you should implement your own buffer.
- Rename `socket::Config` -> `socket::SocketConfig` and add a `heartbeat_ping_msg_fn` member variable in order to support custom Ping/Pong protocols.
    - Add `ClientConfig::socket_config()` setter so clients can define their socket's config.
    - Add `ezsockets::axum::Upgrade::on_upgrade_with_config()` that accepts a `SocketConfig`.
- Refactor `ezeockets::client::connect()` to use a retry loop for the initial connection. Add `max_initial_connect_attempts` and `max_reconnect_attempts` options to the `ClientConfig` (they default to 'infinite').
- Move `axum` and `tungstenite` server runners into new submodule `src/server_runners`.
- Update to `tokio-tungstenite` v0.20.0.
- Fork [axum-tungstenite](https://crates.io/crates/axum-tungstenite) crate into `src/server_runners` and refactor the `axum` runner to use that instead of `axum::extract::ws`.
- Bug fix: remove race condition between sending a message and a socket connection closing that would cause a client to shut down instead of calling `on_disconnect/on_close`.
- Use [`tokio-tungstenite-wasm`](https://github.com/TannerRogalsky/tokio-tungstenite-wasm) errors internally to better support cross-platform clients.
- Use [`enfync`](https://github.com/UkoeHB/enfync) runtime handles internally to better support cross-platform clients. Default clients continue to use tokio.
- Add `ClientConnector` abstraction for connecting clients and add `ezsockets::client::connect_with`.
- Add `ClientConnectorWasm` and `wasm_client` feature. Added `chat-client-wasm` example to show a WASM client that compiles. It currently only listens to the chat and can't input anything since browser does not have a terminal.
- Refactor `Socket` and `Client` to not depend on `tokio` when compiling to WASM. This is a breaking change as the `Client` API now exposes `async_channel` error types instead of `tokio` error types, and `Client::call_with()` now takes an `async_channel::Sender` instead of a `tokio` oneshot.
- Add unimplemented socket close codes.
- Add `ClientExt::on_connect_fail()` for custom handling of connection attempt failures. By default the client will continue trying to connect.


Migration guide:
```rust
impl ezsockets::SessionExt for MySession {
    type Args = (); // <----- 1. Remove Args on `SessionExt`
    // ...
}

impl ezsockets::ServerExt for ChatServer {
    // before
    async fn on_connect(
        &mut self,
        socket: Socket,
        address: SocketAddr,
        args: <Self::Session as ezsockets::SessionExt>::Args, // <----- 2. Remove `args` argument
    ) -> Result<Session, Error> { todo!() }

    // after
    async fn on_connect(
        &mut self,
        socket: Socket,
        request: ezsockets::Request, // <----- 3. Add `request: ezsockets::Request` argument.
        //                                        Note: `ezsockets::Request` is an alias for `http::Request`
        address: SocketAddr,
    ) -> Result<Session, Option<CloseFrame>> { todo!() } // <----- 4. Return `CloseFrame` if rejecting connection.
    
    // ...
}


// ONLY for `axum`

async fn websocket_handler(
    Extension(server): Extension<Server<ChatServer>>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    let session_args = get_session_args();
    // before:
        ezsocket.on_upgrade(server, session_args) // <----- 5. Remove `session_args` argument
    // after:
        ezsocket.on_upgrade(server)               // <----- Now you can customize the `Session` inside of `ServerExt::on_connect` via `ezsockets::Request`.
}

// ONLY for `tungstenite`

// before
ezsockets::tungstenite::run(
    server, 
    "127.0.0.1:8080", 
    |_| async move { Ok(()) } // <----- 6. Remove the last argument, 
                              // Now you can customize the `Session` inside of `ServerExt::on_connect` via `ezsockets::Request`
).await.unwrap();

// after
ezsockets::tungstenite::run(server, "127.0.0.1:8080") // Now you can customize the `Session` inside of `ServerExt::on_connect` via `ezsockets::Request`
    .await
    .unwrap();
```


## v0.5.1
- fix examples links in README.md

## v0.5.0 BREAKING

- feat: add TLS support for servers([#31](https://github.com/gbaranski/ezsockets/pull/31))
- refactor: rename methods and types for better distinction([#28](https://github.com/gbaranski/ezsockets/pull/28))

|Before|After|
|:------|:-----|
|`ClientExt::Params`| `ClientExt::Call`|
|`ClientExt::text(...)`| `ClientExt::on_text(...)`|
|`ClientExt::binary(...)`| `ClientExt::on_binary(...)`|
|`ClientExt::call(...)`| `ClientExt::on_call(...)`|
|`ClientExt::connected(...)`| `ClientExt::on_connect(...)`|
|`ClientExt::closed(...)`| `ClientExt::closed(...)`|
|-|-|
|`ServerExt::Params`| `ServerExt::Call`|
|`ServerExt::accept(...)`| `ServerExt::on_connect(...)`|
|`ServerExt::disconnected(...)`| `ServerExt::on_disconnect(...)`|
|`ServerExt::call(...)`| `ServerExt::on_call(...)`|
|-|-|
|`SessionExt::Params`| `SessionExt::Call`|
|`SessionExt::text(...)`| `SessionExt::on_text(...)`|
|`SessionExt::binary(...)`| `SessionExt::on_binary(...)`|
|`SessionExt::call(...)`| `SessionExt::on_call(...)`|

Additionally for the `ezsockets::tungstenite::run_on` function you need to also pass an `ezsockets::tungstenite::Acceptor`, if you don't use TLS, just pass `ezsockets::tungstenite::Acceptor::Plain`.


## v0.4.3
- connect request customizability([#25](https://github.com/gbaranski/ezsockets/pull/25)) with bearer authorization and custom headers.
- allow passing String directly to `ClientConfig::new()`

## v0.4.2
- client connection handlers. `ClientExt::closed()` and `ClientExt::connected()`

## v0.4.1
- add `Client::close()` method
- add benchmarks
- use biased non-random `tokio::select`

## v0.4.0 BREAKING
- add `alive()` to check if connection is alive.
- add `counter-server` example
- more friendly panic messages
- make `Server::call`, `Session::text`, `Session::binary` and `Session::call` synchronous

## v0.3.0 BREAKING
- Automatic GitHub workflows
- Integration tests
- Add `tungstenite::run_on` for alternative setups
- Refactor the `Client<T>` `T` generic logic

## v0.2.0 BREAKING
- fix panics
- close connection on heartbeat failure
- improve reconnection mechanism
- add basic authorization
- rename `Client::message()` to `Client::call()`
- `Client::call()` returns `Result<T, E>`
- To/From Sender conversions on Client
- Custom calls on `Session`
- Add `Args` for session creation
- Add `call_with` for with-response call
- improve logging
- remove `thiserror` usage

https://github.com/gbaranski/ezsockets/compare/v0.1.0...v0.2.0

## v0.1.0

First release!
