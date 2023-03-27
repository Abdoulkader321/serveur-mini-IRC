// cargo add --path ../mini-irc-protocol

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
use std::error::Error;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;

// hashmap

enum AskRessource {
    JoinServer(String, oneshot::Sender<bool>),
}


async fn respond_to_client(writer: OwnedWriteHalf, object_to_send: Response) {
    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
    typed_writer.send(&object_to_send).await.unwrap();
}

async fn warn_client_about_an_error(writer: OwnedWriteHalf, error_msg: String) {
    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
    typed_writer
        .send(&Response::Error(error_msg))
        .await
        .unwrap();
}

async fn server_database(rx: &mut Receiver<AskRessource>, connected_clients: &mut Vec<String>) {
    while let Some(AskRessource::JoinServer(username, resp)) = rx.recv().await {
        if connected_clients.iter().any(|s| s == &username) {
            if let Err(e) = resp.send(false) {
                println!("!! An error occured in send: {e} !!");
            }
        } else {
            if let Err(e) = resp.send(true) {
                println!("!! An error occured in send: {e} !!");
            }
            connected_clients.push(username.clone());
        }
    }
}

async fn handle_clients(socket: TcpStream, thread_tx: &mut Sender<AskRessource>) {
    let (reader, writer) = socket.into_split();
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    match typed_reader.recv().await {
        Ok(Some(connection)) => {
            if let Request::Connect(username) = connection {
                let (tx, rx) = oneshot::channel();
                let ressource = AskRessource::JoinServer(username.clone(), tx);

                match thread_tx.send(ressource).await {
                    Ok(_) => match rx.await {
                        Ok(response) => {
                            if response {
                                println!("-- Connection from {} accepted --\n", username);

                                /* Respond to client that he is accepted on the server */
                                respond_to_client(writer, Response::AckConnect(username)).await;
                            } else {
                                println!("-- Connection from {} refused --\n", username);
                                
                                warn_client_about_an_error(
                                    writer,
                                    "Another user with the same name already exist!".to_string(),
                                )
                                .await;
                                return ;
                            }
                        }
                        Err(e) => {
                            println!("!! An error occured in receiving : {e} !!");
                            return ;
                        }
                    },
                    Err(e) => {
                        println!("!! An error occured in send: {e} !!");
                        return ;
                    }
                }
            } else {
                println!("-- One user do not respect the protocol : quicked out --\n");
                warn_client_about_an_error(writer, "You must respect the protocol".to_string())
                    .await;
                return ;
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

    let (tx, mut rx): (Sender<AskRessource>, Receiver<AskRessource>) = mpsc::channel(100);

    tokio::spawn(async move {
        server_database(&mut rx, &mut connected_clients).await;
    });

    loop {
        let (socket, _) = listener.accept().await?;
        let mut thread_tx = tx.clone();

        tokio::spawn(async move {
            handle_clients(socket, &mut thread_tx).await;
        });
    }
}
