# Change Log
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

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