use std::{
    net::{TcpListener, TcpStream},
    thread::spawn,
};

use criterion::{criterion_group, criterion_main, Bencher, Criterion};
use rand::Rng;
use tokio_tungstenite::tungstenite::{self, stream::MaybeTlsStream, WebSocket};

mod ezsockets_server;
mod tungstenite_server;

fn bench(b: &mut Bencher, client: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let mut rng = rand::thread_rng();
    b.iter(|| {
        let nonce = rng.gen::<u32>();
        let text = format!("Hello {}", nonce);
        let message = tungstenite::Message::Text(text.clone());
        client.write_message(message).unwrap();

        while let tungstenite::Message::Text(received_text) = client.read_message().unwrap() {
            return Some(received_text);
        }
        return None;
    });
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut group = c.benchmark_group("read latency");

    let port = 4321;
    let task = spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        tungstenite_server::run(listener)
    });

    let mut client = tungstenite::connect(format!("ws://127.0.0.1:{}", port))
        .unwrap()
        .0;
    group.bench_function("tungstenite server", |b| bench(b, &mut client));
    client.close(None).unwrap();

    let task = runtime.spawn(async move {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        ezsockets_server::run(listener).await;
    });
    // group.bench_function("ezsockets server", |b| bench(b, port));
    task.abort();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
