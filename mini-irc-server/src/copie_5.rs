// cargo add --path ../mini-irc-protocol

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
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

async fn handle_incoming_connection(socket: TcpStream, thread_tx: &mut Sender<String>) {
    let (reader, mut writer) = socket.into_split();
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    match typed_reader.recv().await {
        Ok(Some(connection)) => {
            if let Request::Connect(username) = connection {

                match thread_tx.send(username).await {
                    Ok(_) => {
                       
                    },
                    Err(e) => {
                        println!("!! An error occured in send !!");
                        println!("{e}");                    
                    }
                }

            } else {
                println!("-- You must respect the protocol \n");

                let mut typed_writer = AsyncTypedWriter::<_, Response>::new(&mut writer);
                typed_writer
                    .send(&Response::Error(
                        "You must respect the protocol".to_string(),
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
            handle_incoming_connection(socket, &mut thread_tx).await;
        });
    }
}
