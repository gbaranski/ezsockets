use tokio_tungstenite::tungstenite::{accept, Message};

pub fn run(listener: std::net::TcpListener) {
    for (stream, _address) in listener.accept() {
        let mut websocket = accept(stream).unwrap();
        loop {
            let message = websocket.read_message().unwrap();
            // println!("server | msg: {:?}", &message);
            match message {
                Message::Text(text) => websocket.write_message(Message::Text(text)).unwrap(),
                Message::Binary(_) => todo!(),
                Message::Ping(_) => todo!(),
                Message::Pong(_) => todo!(),
                Message::Close(_) => return,
                Message::Frame(_) => todo!(),
            }
        }
    }
}
