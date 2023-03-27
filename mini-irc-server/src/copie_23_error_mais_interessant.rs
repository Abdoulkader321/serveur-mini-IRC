// cargo add --path ../mini-irc-protocol
// cargo clippy

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::BroadcastReceiverWithList;
use mini_irc_protocol::BroadcastSenderWithList;
use mini_irc_protocol::ChanOp;
use mini_irc_protocol::MessageReceiver;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
use std::error::Error;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;

// hashmap

enum AskRessource {
    JoinServer(String, oneshot::Sender<bool>),
    AskToJoinChannel(
        String,
        String,
        oneshot::Sender<BroadcastReceiverWithList<Response, String>>,
    ),
    AskToLeaveChannel(String, String),
    UserDisconnected(String),
    TransferMessageToChannel(String, String, String, oneshot::Sender<bool>)
}

struct Channel {
    name: String,
    sender: BroadcastSenderWithList<Response, String>,
}

fn check_channel_exists(channels: Vec<Channel>, channel_name: String) -> bool{

    false
}   

fn check_user_in_channel(channels: Vec<Channel>, channel_name: String, username: String) -> bool{

    false
}  

async fn handle_writer(writer: &mut OwnedWriteHalf, rx: &mut Receiver<Response>) {
    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);

    while let Some(object_to_send) = rx.recv().await {
        typed_writer.send(&object_to_send).await.unwrap();
    }
}

async fn respond_to_client(writer_tx: &mut Sender<Response>, object_to_send: Response) {
    if writer_tx.send(object_to_send).await.is_err() {
        println!("!! An error occured !!");
    }
}

async fn server_database(rx: &mut Receiver<AskRessource>) {
    let mut connected_clients: Vec<String> = Vec::new(); // Database
    let mut channels: Vec<Channel> = Vec::new(); // Database

    loop {
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
            }
            Some(AskRessource::AskToJoinChannel(channel_name, username, resp)) => {
                match channels
                    .iter()
                    .position(|channel| channel.name == channel_name)
                {
                    Some(index) => {
                        let receiver = channels[index].sender.subscribe(username.clone());

                        if resp.send(receiver).is_err() {
                            println!("!! An error occured !!");
                        }
                    }
                    None => {
                        // Channel created

                        let mut new_sender: BroadcastSenderWithList<Response, String> =
                            BroadcastSenderWithList::new(50);
                        let receiver = new_sender.subscribe(username.clone());

                        let new_channel = Channel {
                            name: channel_name,
                            sender: new_sender,
                        };

                        channels.push(new_channel);

                        if resp.send(receiver).is_err() {
                            println!("!! An error occured !!");
                        }
                    }
                }
            }

            Some(AskRessource::UserDisconnected(username)) => {
                let index = connected_clients
                    .iter()
                    .position(|name| *name == username)
                    .unwrap();
                connected_clients.remove(index);

                // todo A gerer, le retirer du channel aussi
            },

            Some(AskRessource::TransferMessageToChannel(username, channel_name, content, resp)) => {

                let mut found = false;

                let channel_index = channels.iter().position(|channel| channel.name == channel_name);

                if let Some(index) = channel_index {
                    
                    check_user_in_channel(channels, "ab".to_string(), "ab".to_string());

                   if channels[index].sender.into_subscribers().iter().any(|name| *name == username) {
                        found = true;
                        
                        let op = ChanOp::Message { from: username, content };                
                        let ressource = Response::Channel { op, chan: channel_name };

                        if channels[index].sender.send(ressource).is_err(){
                            println!("!! An error occured !!\n");
                        }
                   }

                }                  
                
                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n");
                }

            },
            Some(AskRessource::AskToLeaveChannel(username, channel_name)) => {

            }
            _ => {
                println!("!! An error occured !!");
            }
        }
    }
}

async fn handle_waiting_for_messages(
    username: String,
    broadcast_receiver: &mut BroadcastReceiverWithList<Response, String>,
    writer_tx: &mut Sender<Response>,
) {
    let mut finished = false;
    while !finished {
        match broadcast_receiver.recv().await {
            Ok(Response::Channel { op, chan }) => {
                respond_to_client(writer_tx, Response::Channel { op, chan }).await;
            }
            Ok(Response::AckLeave(chan_name)) => {
                finished = true;
                respond_to_client(writer_tx, Response::AckLeave(chan_name)).await;
            }
            _ => {}
        }
    }
}

async fn handle_client(
    username: String,
    reader: &mut OwnedReadHalf,
    writer_tx: &mut Sender<Response>,
    thread_tx: &mut Sender<AskRessource>,
) {
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    loop {
        match typed_reader.recv().await {
            Ok(Some(Request::JoinChan(channel_name))) => {
                // The user wants to join a channel

                let (tx, rx) = oneshot::channel();
                let ressource =
                    AskRessource::AskToJoinChannel(channel_name.clone(), username.clone(), tx);

                match thread_tx.send(ressource).await {
                    Ok(_) => match rx.await {
                        Ok(mut broadcast_receiver) => {
                            respond_to_client(
                                writer_tx,
                                Response::AckJoin {
                                    chan: channel_name.clone(),
                                    users: broadcast_receiver.into_subscribers(),
                                },
                            )
                            .await;

                            let mut copy_writer_tx = writer_tx.clone();
                            let copy_username = username.clone();

                            tokio::spawn(async move {
                                handle_waiting_for_messages(
                                    copy_username,
                                    &mut broadcast_receiver,
                                    &mut copy_writer_tx,
                                )
                                .await;
                            });
                        }
                        Err(e) => {
                            println!("!! An error occured {e} !!");
                        }
                    },
                    _ => {
                        println!("!! An error occured !!");
                    }
                }
            }

            Ok(Some(Request::Message { to, content })) => match to {
                MessageReceiver::Channel(channel_name) => {
                    let (tx, rx) = oneshot::channel();
                    let ressource =
                    AskRessource::TransferMessageToChannel(username.clone(), channel_name.clone(), content, tx);

                    match thread_tx.send(ressource).await {
                        Ok(_) => match rx.await {
                            Ok(false) => {
                                respond_to_client(
                                    writer_tx,
                                    Response::Error(
                                        "You must ask to join to channel OR channel not found"
                                            .to_string(),
                                    ),
                                ).await;
                            }
                            Ok(true) => {
                                println!("{} sent a message in the channel {} \n", username.clone(), channel_name);
                            }
                            Err(_) => {
                                println!("!! An error occured in send !!");
                            }
                        },
                        Err(e) => {
                            println!("!! An error occured in send: {e} !!");
                        }
                    }
                }
                _ => {
                    println!("Not yet implemented \n");
                }
            },
            Ok(Some(Request::LeaveChan(channel_name))) => {
                let ressource = AskRessource::AskToLeaveChannel(username.clone(), channel_name.clone());

                match thread_tx.send(ressource).await{
                    
                    Ok(_) => {
                        println!("{} left the channel {}", username.clone(), channel_name);
                    }, 
                    Err(_) => {
                        println!("!! An error occured in send: !!");
                    }

                }


            }
            Ok(None) | Err(_) => {
                let ressource = AskRessource::UserDisconnected(username.clone());
                match thread_tx.send(ressource).await {
                    Ok(_) => {
                        println!("-- {} left the server -- \n", username);
                    }
                    _ => {
                        println!("!! An error occured !!");
                    }
                }

                return;
            }
            _ => {
                println!("Not yet covered \n");
            }
        }
    }
}

async fn is_client_accepted(
    reader: &mut OwnedReadHalf,
    writer_tx: &mut Sender<Response>,
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
                                respond_to_client(
                                    writer_tx,
                                    Response::AckConnect(username.clone()),
                                )
                                .await;

                                return Some(username.clone());
                            } else {
                                println!("-- Connection from {} refused --\n", username);

                                respond_to_client(
                                    writer_tx,
                                    Response::Error(
                                        "Another user with the same name already exist!"
                                            .to_string(),
                                    ),
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
                respond_to_client(
                    writer_tx,
                    Response::Error("You must respect the protocol".to_string()),
                )
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

        let (mut reader, mut writer) = socket.into_split();

        let (tx_writer, mut rx_writer): (Sender<Response>, Receiver<Response>) = mpsc::channel(100);

        tokio::spawn(async move {
            handle_writer(&mut writer, &mut rx_writer).await;
        });

        tokio::spawn(async move {
            if let Some(username) =
                is_client_accepted(&mut reader, &mut tx_writer.clone(), &mut thread_tx).await
            {
                handle_client(
                    username,
                    &mut reader,
                    &mut tx_writer.clone(),
                    &mut thread_tx,
                )
                .await;
            }
        });
    }
}
