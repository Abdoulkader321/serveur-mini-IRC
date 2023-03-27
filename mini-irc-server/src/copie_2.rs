// cargo add --path ../mini-irc-protocol

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
use std::error::Error;
use std::io;
use std::ops::Index;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::stream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};

use std::sync::Arc;
use tokio::sync::Mutex;

// let count = Arc::new(Mutex::new(0));

// let mut shared_data = Arc::new(Mutex::new(Vec::new()));

// let mut connected_clients: Vec<String> = Vec::new();

// Arc mutex mpsc channel

// async fn handle_connection(socket: TcpStream, connected_clients:<Arc<Mutex<Vec<>>>>) {}

// hashmap

#[tokio::main]

async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hello, world!");

    let (tx, mut rx): (Sender<String>, Receiver<String>) = mpsc::channel(100);
    let mut database_connected_clients: Vec<String> = Vec::new();

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    tokio::spawn(async move {
        while let Some(new_user) = rx.recv().await {
            print!("-- Connection from {} ", new_user);

            if (database_connected_clients.iter().find(|&s| s == &new_user).is_some()) {
                println!("refused --\n");
            } else {
                println!("accepted --\n");
                database_connected_clients.push(new_user.clone());

                let stream_response = TcpStream::connect("127.0.0.1:8080").await.unwrap();
                let (reader, writer) = stream_response.into_split();
                let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
                typed_writer
                    .send(&Response::AckConnect(new_user))
                    .await
                    .unwrap();
            }
        }
    });

    loop {
        let (socket, _) = listener.accept().await?;
        let thread_tx = tx.clone();

        tokio::spawn(async move {

            let (reader, writer) = socket.into_split();
            let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

            loop {
                let connection_request: Request = typed_reader.recv().await.unwrap()
                .unwrap();

                if let Request::Connect(username) = connection_request {
                    print!("-- Connection from {} ", username);

                    thread_tx.send(username).await;

                } else {
                    println!("-- You must respect the protocol \n");

                    let stream_response = TcpStream::connect("127.0.0.1:8080").await.unwrap();
                    let (reader, writer) = stream_response.into_split();
                    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
                    typed_writer
                        .send(&Response::Error(
                            "-- You must respect the protocol \n".to_string(),
                        ))
                        .await
                        .unwrap();
                }
            }
        });


    }

    Ok(())
}
