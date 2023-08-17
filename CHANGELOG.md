# Change Log
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

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
        ezsocket.on_upgrade(server)               // <----- Now you can customize the `Session` inside of `ServerExt::on_connect` via `ezsockets::Request`
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
