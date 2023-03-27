// cargo add --path ../mini-irc-protocol

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
use std::error::Error;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;

// hashmap

enum AskRessource {
    JoinServer(String, oneshot::Sender<bool>),
    ChannelExist(String, oneshot::Sender<bool>),
}

struct Channel {
    name: String,
    users: Vec<(String, Arc<OwnedWriteHalf>)>,
}

async fn respond_to_client(writer: &mut OwnedWriteHalf, object_to_send: Response) {
    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
    typed_writer.send(&object_to_send).await.unwrap();
}

async fn warn_client_about_an_error(writer: &mut OwnedWriteHalf, error_msg: String) {
    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
    typed_writer
        .send(&Response::Error(error_msg))
        .await
        .unwrap();
}

async fn server_database(rx: &mut Receiver<AskRessource>) {
    let mut connected_clients: Vec<String> = Vec::new(); // Database
    let mut channels: Vec<Channel> = Vec::new(); // Database

    
    loop {

        /*
        match rx.recv().await {
            Some(AskRessource::JoinServer(username, resp)) => {
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
            },
            Some(AskRessource::ChannelExist(channel_name, resp)) => {

                for channel in channels.iter() {

                    if channel.name == channel_name {
                        resp.send(true);
                    }

                } 

            },

            _ => {
                println!();
            }
        }
        */

        let x = rx.recv().await;

        if let Some(AskRessource::JoinServer(username, resp)) = x {
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

        else if let Some(AskRessource::ChannelExist(channel_name, resp)) = x {

        }

    }
}

async fn handle_client(
    username: String,
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    thread_tx: &mut Sender<AskRessource>,
) {
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);
    let writer_arc = Arc::new(writer);

    loop {
        match typed_reader.recv().await {
            Ok(Some(Request::JoinChan(channel_name))) => {
                // The user wants to join a channel
                let (tx, rx) = oneshot::channel();
                let ressource = AskRessource::ChannelExist(channel_name.clone(), tx);

                match thread_tx.send(ressource).await {
                    Ok(_) => match rx.await {
                        Ok(channel_exist) => {
                            let channel = Channel {
                                name: channel_name.clone(),
                                users: vec![(username.clone(), Arc::clone(&writer_arc))],
                            };
                        }
                        Err(e) => {
                            println!("!! An error occured {e} !!");
                        }
                    },
                    _ => {
                        println!();
                    }
                }
            }
            _ => {
                println!();
            }
        }
    }
}

async fn is_client_accepted(
    reader: &mut OwnedReadHalf,
    writer: &mut OwnedWriteHalf,
    thread_tx: &mut Sender<AskRessource>,
) -> Option<String> {
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
                                respond_to_client(writer, Response::AckConnect(username.clone()))
                                    .await;
                                return Some(username.clone());
                            } else {
                                println!("-- Connection from {} refused --\n", username);

                                warn_client_about_an_error(
                                    writer,
                                    "Another user with the same name already exist!".to_string(),
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            println!("!! An error occured in receiving : {e} !!");
                        }
                    },
                    Err(e) => {
                        println!("!! An error occured in send: {e} !!");
                    }
                }
            } else {
                println!("-- One user do not respect the protocol : quicked out --\n");
                warn_client_about_an_error(writer, "You must respect the protocol".to_string())
                    .await;
            }
        }
        _ => {
            println!("!! An error occured: !!");
        }
    }

    None
}

#[tokio::main]

async fn main() -> Result<(), Box<dyn Error>> {
    println!("-- Welcome on the server --\n");

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    let (tx, mut rx): (Sender<AskRessource>, Receiver<AskRessource>) = mpsc::channel(100);

    tokio::spawn(async move {
        server_database(&mut rx).await;
    });

    loop {
        let (socket, _) = listener.accept().await?;
        let mut thread_tx = tx.clone();

        tokio::spawn(async move {
            let (mut reader, mut writer) = socket.into_split();
            match is_client_accepted(&mut reader, &mut writer, &mut thread_tx).await {
                Some(username) => {
                    handle_client(username, reader, writer, &mut thread_tx).await;
                }
                None => {
                    println!();
                }
            }
        });
    }
}
