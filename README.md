# ezsockets

Have you ever tried building a WebSocket server or a client in Rust? It might be pretty hard, especially if you want to accept calls from outside of the WebSocket connection or sending Ping messages, while keeping it concurrent. Now you can use ezsockets to make this process much easier, focusing on handling messages in a event-based way.