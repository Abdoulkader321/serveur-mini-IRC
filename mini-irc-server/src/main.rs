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

/* todo Suppression du Channel privé d'un client */

/* feat: Un utilisateur ne peut pas envoyer de message privé à lui même */

enum AskRessource {
    JoinServer(String, oneshot::Sender<bool>),
    CreatePrivateChannel(
        String,
        oneshot::Sender<BroadcastReceiverWithList<BroadcastMessage, String>>,
    ),
    AskToJoinChannel(
        String,
        String,
        oneshot::Sender<BroadcastReceiverWithList<BroadcastMessage, String>>,
    ),
    AskToLeaveChannel(String, String, oneshot::Sender<bool>),
    UserDisconnected(String),
    TransferMessageToChannel(String, String, String, oneshot::Sender<bool>),
    TransferMessageToAClient(String, String, String, oneshot::Sender<bool>),
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum BroadcastMessage {
    MessageToChannel(String, String, String),
    DirectMessage(String, String),
    /// Ack de sortie d'un channel.
    Leave(String, String, bool),
    NewUserJoin(String, String),
}

struct Channel {
    name: String,
    sender: BroadcastSenderWithList<BroadcastMessage, String>,
}

fn check_channel_exists(channels: &[Channel], channel_name: String) -> Option<usize> {
    channels
        .iter()
        .position(|channel| channel.name == channel_name)
}

fn check_user_in_channel(
    channels: &[Channel],
    channel_index: usize,
    username: String,
) -> Option<usize> {
    channels[channel_index]
        .sender
        .into_subscribers()
        .iter()
        .position(|name| *name == username)
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

            Some(AskRessource::CreatePrivateChannel(username, resp)) => {
                let mut new_sender: BroadcastSenderWithList<BroadcastMessage, String> =
                    BroadcastSenderWithList::new(50);
                let receiver = new_sender.subscribe(username.clone());

                let formatted_channel_name = format!("{username}-privet-channel");
                let new_channel = Channel {
                    name: formatted_channel_name,
                    sender: new_sender,
                };

                if resp.send(receiver).is_err() {
                    println!("!! An error occured !!");
                } else {
                    channels.push(new_channel);
                }
            }

            Some(AskRessource::AskToJoinChannel(channel_name, username, resp)) => {
                match check_channel_exists(&channels, channel_name.clone()) {
                    Some(index) => {
                        let receiver = channels[index].sender.subscribe(username.clone());

                        if resp.send(receiver).is_err() {
                            println!("!! An error occured !!");
                        }

                        // Informer les autres utilisateurs
                        let ressource =
                            BroadcastMessage::NewUserJoin(username.clone(), channel_name);
                        if channels[index].sender.send(ressource).is_err() {
                            println!("!! An error occured !!");
                        }
                    }
                    None => {
                        // Channel created

                        let mut new_sender: BroadcastSenderWithList<BroadcastMessage, String> =
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
                /* Remove user for connected clients array */
                let index = connected_clients
                    .iter()
                    .position(|name| *name == username)
                    .unwrap();

                connected_clients.remove(index);

                /* Remove User from channels && remove channel if there is no user*/
                for channel_index in 0..channels.len() {

                    let private_channel_name = format!("{username}-privet-channel");
                    if check_user_in_channel(&channels, channel_index, username.clone()).is_some() || channels[channel_index].name == private_channel_name{
                        let ressource = BroadcastMessage::Leave(
                            username.clone(),
                            channels[channel_index].name.clone(),
                            true,
                        );
                        if channels[channel_index].sender.send(ressource).is_err() {
                            println!("!! An error occured !!\n");
                        }

                        if channels[channel_index].sender.into_subscribers().is_empty() {
                            channels.swap_remove(channel_index);
                        }
                    }
                }

                for channel_index in 0..channels.len() {
                    println!("--> {}", channels[channel_index].name);
                }
            }

            Some(AskRessource::TransferMessageToChannel(username, channel_name, content, resp)) => {
                let mut found = false;

                let channel_index = check_channel_exists(&channels, channel_name.clone());

                if let Some(index) = channel_index {
                    if check_user_in_channel(&channels, index, username.clone()).is_some() {
                        found = true;
                        let ressource =
                            BroadcastMessage::MessageToChannel(username, channel_name, content);

                        if channels[index].sender.send(ressource).is_err() {
                            println!("!! An error occured !!\n");
                        }
                    }
                }

                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n");
                }
            }

            Some(AskRessource::TransferMessageToAClient(
                sender_name,
                receiver_name,
                message,
                resp,
            )) => {
                let mut found = false;
                let channel_name = format!("{receiver_name}-privet-channel");

                let channel_index = check_channel_exists(&channels, channel_name.clone());

                if let Some(index) = channel_index {
                    found = true;

                    let ressource =
                        BroadcastMessage::DirectMessage(sender_name, message);

                    if channels[index].sender.send(ressource).is_err() {
                        println!("!! An error occured !!\n");
                    }
                }

                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n");
                }

            }

            Some(AskRessource::AskToLeaveChannel(username, channel_name, resp)) => {
                let mut found = false;
                let channel_index = check_channel_exists(&channels, channel_name.clone());

                if let Some(index) = channel_index {
                    if check_user_in_channel(&channels, index, username.clone()).is_some() {
                        found = true;

                        let ressource = BroadcastMessage::Leave(username, channel_name, false);

                        if channels[index].sender.send(ressource).is_err() {
                            println!("!! An error occured !!\n");
                        }

                        if channels[index].sender.into_subscribers().is_empty() {
                            channels.swap_remove(index);
                        }
                    }
                }

                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n");
                }
            }
            _ => {
                println!("!! An error occured !!");
            }
        }
    }
}

async fn handle_waiting_for_messages(
    username: String,
    broadcast_receiver: &mut BroadcastReceiverWithList<BroadcastMessage, String>,
    writer_tx: &mut Sender<Response>,
) {
    let mut finished = false;
    while !finished {
        match broadcast_receiver.recv().await {
            Ok(BroadcastMessage::MessageToChannel(user_name, chan, content)) => {
                let op = ChanOp::Message {
                    from: user_name,
                    content,
                };

                respond_to_client(writer_tx, Response::Channel { op, chan }).await;
            }

            Ok(BroadcastMessage::DirectMessage(sender_name, message)) => {
                respond_to_client(writer_tx, Response::DirectMessage { from: sender_name, content:message}).await;
            }

            Ok(BroadcastMessage::Leave(user_name, channel, suddenly_interrupt)) => {
                /* Current user wants to leave */

                if user_name == username {
                    finished = true;

                    if !suddenly_interrupt {
                        respond_to_client(writer_tx, Response::AckLeave(channel)).await;
                    }
                } else {
                    let chan_op = ChanOp::UserDel(user_name);
                    respond_to_client(
                        writer_tx,
                        Response::Channel {
                            op: chan_op,
                            chan: channel,
                        },
                    )
                    .await;
                }
            }
            Ok(BroadcastMessage::NewUserJoin(user_name, chan)) => {
                if username != user_name {
                    let op = ChanOp::UserAdd(user_name);
                    respond_to_client(writer_tx, Response::Channel { op, chan }).await;
                }
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
                                drop(broadcast_receiver);
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
                    let ressource = AskRessource::TransferMessageToChannel(
                        username.clone(),
                        channel_name.clone(),
                        content,
                        tx,
                    );

                    match thread_tx.send(ressource).await {
                        Ok(_) => match rx.await {
                            Ok(false) => {
                                respond_to_client(
                                    writer_tx,
                                    Response::Error(
                                        "You must ask to join to channel OR channel not found"
                                            .to_string(),
                                    ),
                                )
                                .await;
                            }
                            Ok(true) => {
                                println!(
                                    "{} sent a message in the channel {} \n",
                                    username.clone(),
                                    channel_name
                                );
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
                MessageReceiver::User(receiver_name) => {
                    let (tx, rx) = oneshot::channel();
                    let ressource = AskRessource::TransferMessageToAClient(
                        username.clone(),
                        receiver_name.clone(),
                        content,
                        tx,
                    );

                    match thread_tx.send(ressource).await {
                        Ok(_) => match rx.await {
                            Ok(true) => {
                                println!(
                                    "{} sent a private message to {} \n",
                                    username.clone(),
                                    receiver_name.clone()
                                );
                            }

                            Ok(false) => {
                                respond_to_client(
                                    writer_tx,
                                    Response::Error(
                                        "The receiver is not found on the server".to_string(),
                                    ),
                                )
                                .await;
                            }
                            Err(e) => {
                                println!("!! An error occured in send: {e} !!");
                            }
                        },

                        Err(e) => {
                            println!("!! An error occured in send: {e} !!");
                        }
                    }
                }
            },
            Ok(Some(Request::LeaveChan(channel_name))) => {
                let (tx, rx) = oneshot::channel();

                let ressource =
                    AskRessource::AskToLeaveChannel(username.clone(), channel_name.clone(), tx);

                match thread_tx.send(ressource).await {
                    Ok(_) => match rx.await {
                        Ok(true) => {
                            println!("{} left the channel {}", username.clone(), channel_name);
                        }
                        _ => {
                            respond_to_client(
                                writer_tx,
                                Response::Error(
                                    "You must ask to join to channel OR channel not found"
                                        .to_string(),
                                ),
                            )
                            .await;
                        }
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

                                /* Create a channel for Client so that he could receive private message */
                                let (tx, rx) = oneshot::channel();
                                let ressource =
                                    AskRessource::CreatePrivateChannel(username.clone(), tx);
                                match thread_tx.send(ressource).await {
                                    Ok(_) => match rx.await {
                                        Ok(mut broadcast_receiver) => {
                                            let mut copy_writer_tx = writer_tx.clone();
                                            let copy_username = username.clone();

                                            tokio::spawn(async move {
                                                handle_waiting_for_messages(
                                                    copy_username,
                                                    &mut broadcast_receiver,
                                                    &mut copy_writer_tx,
                                                )
                                                .await;
                                                drop(broadcast_receiver);
                                            });
                                        }

                                        Err(e) => {
                                            println!("!! An error occured in creating a private channel : {e} !!");
                                        }
                                    },

                                    Err(e) => {
                                        println!("!! An error occured in creating a private channel : {e} !!");
                                    }
                                }

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

    let listener = TcpListener::bind("127.0.0.1:2027").await?;

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
