use tokio_tungstenite::tungstenite::{accept, Message};

pub fn run(listener: std::net::TcpListener) {
    while let Ok((stream, _address)) = listener.accept() {
        let mut websocket = accept(stream).unwrap();
        loop {
            let message = websocket.read().unwrap();
            // println!("server | msg: {:?}", &message);
            match message {
                Message::Text(text) => websocket.send(Message::Text(text)).unwrap(),
                Message::Binary(_) => todo!(),
                Message::Ping(_) => todo!(),
                Message::Pong(_) => todo!(),
                Message::Close(_) => return,
                Message::Frame(_) => todo!(),
            }
        }
    }
}
