// cargo add --path ../mini-irc-protocol

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use std::error::Error;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};

// hashmap

async fn handle_clients(rx: &mut Receiver<String>, connected_clients: &mut Vec<String>) {
    while let Some(username) = rx.recv().await {
        print!("-- Connection from {} ", username);

        if connected_clients.iter().any(|s| s == &username) {
            println!("refused --\n");
        } else {
            println!("accepted --\n");
            connected_clients.push(username.clone());

            /* Respond to client that he is accepted on the server */
            let stream_response = TcpStream::connect("127.0.0.1:8080").await.unwrap();
            let (_, writer) = stream_response.into_split();
            let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
            typed_writer
                .send(&Response::AckConnect(username))
                .await
                .unwrap();
        }
    }
}

async fn handle_incoming_connection(reader: &mut OwnedReadHalf, writer: &mut OwnedWriteHalf, thread_tx: &mut Sender<String>) {
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    loop {
        match typed_reader.recv().await {
            Ok(Some(connection_request)) => {
                if let Request::Connect(username) = connection_request {
                    //print!("-- Connection from {} ", username);

                    thread_tx.send(username).await;
                } else {
                    println!("-- You must respect the protocol \n");

                    //let stream_response = TcpStream::connect("127.0.0.1:8080").await.unwrap();
                    //let (reader, writer) = stream_response.into_split();
                    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(&mut *writer);
                    typed_writer
                        .send(&Response::Error(
                            "-- You must respect the protocol \n".to_string(),
                        ))
                        .await
                        .unwrap();
                }
            }
            _ => {
                return;
            }
        }
    }
}

#[tokio::main]

async fn main() -> Result<(), Box<dyn Error>> {
    println!("-- Welcome on the server --\n");

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    let mut connected_clients: Vec<String> = Vec::new(); // Database
    let (tx, mut rx): (Sender<String>, Receiver<String>) = mpsc::channel(100);

    tokio::spawn(async move {
        handle_clients(&mut rx, &mut connected_clients).await;
    });

    loop {
        let (socket, _) = listener.accept().await?;
        let mut thread_tx = tx.clone();

        tokio::spawn(async move {
            let (mut reader, mut writer) = socket.into_split();
            handle_incoming_connection(&mut reader, &mut writer, &mut thread_tx).await;
        });
    }
}
