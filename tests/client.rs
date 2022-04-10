use std::{net::SocketAddr, str::FromStr};

use ezsockets::{Client, ClientConfig, ClientExt};
use url::Url;

pub async fn connect<E: ClientExt + 'static>(
    client_fn: impl FnOnce(Client<E>) -> E,
    address: SocketAddr,
) -> Client<E> {
    let url = format!("ws://{}/websocket", address);
    let url = Url::from_str(&url).unwrap();
    let (client, _) = ezsockets::connect(client_fn, ClientConfig::new(url)).await;
    client
}
