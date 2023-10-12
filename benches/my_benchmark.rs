use std::{
    net::{TcpListener, TcpStream},
    thread::sleep,
    thread::spawn,
    time::Duration,
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
        client.send(message).unwrap();

        while let tungstenite::Message::Text(received_text) = client.read().unwrap() {
            return Some(received_text);
        }
        return None;
    });
}

pub fn criterion_benchmark(c: &mut Criterion) {
    tracing_subscriber::fmt::init();
    let mut group = c.benchmark_group("read latency");

    let port = 4321;
    let thread = spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        tungstenite_server::run(listener)
    });

    let mut client = tungstenite::connect(format!("ws://127.0.0.1:{}", port))
        .unwrap()
        .0;
    group.bench_function("tungstenite server", |b| bench(b, &mut client));
    client.close(None).unwrap();
    thread.join().unwrap();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let task = runtime.spawn(async move {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        ezsockets_server::run(listener).await;
    });
    sleep(Duration::from_millis(100));
    let mut client = tungstenite::connect(format!("ws://127.0.0.1:{}", port))
        .unwrap()
        .0;
    group.bench_function("ezsockets server", |b| bench(b, &mut client));
    client.close(None).unwrap();
    task.abort();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = criterion_benchmark
}
criterion_main!(benches);
