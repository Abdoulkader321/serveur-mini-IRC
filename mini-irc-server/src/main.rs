// cargo add --path ../mini-irc-protocol
// cargo clippy

use mini_irc_protocol::AsyncTypedReader;
use mini_irc_protocol::AsyncTypedWriter;
use mini_irc_protocol::BroadcastReceiverWithList;
use mini_irc_protocol::BroadcastSenderWithList;
use mini_irc_protocol::Chan;
use mini_irc_protocol::ChanOp;
use mini_irc_protocol::ErrorType;
use mini_irc_protocol::Key;
use mini_irc_protocol::MessageReceiver;
use mini_irc_protocol::Request;
use mini_irc_protocol::Response;
use mini_irc_protocol::ResponsePlusKey;
use rand_core::OsRng;
use std::error::Error;
use std::str::FromStr;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use x25519_dalek::{EphemeralSecret, PublicKey};

/* todo  */

/* feat: + Un utilisateur est entrain d'ecrire dans tel channel,
+ Données dans un fichier
+ tous les cas sont bien gere normalement */

enum AskRessource {
    JoinServer(String, String, oneshot::Sender<bool>),
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
    NotifClientIsWriting(String, Chan),
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum BroadcastMessage {
    MessageToChannel(String, String, String),
    DirectMessage(String, String),
    /// Ack de sortie d'un channel.
    Leave(String, String, bool),
    NewUserJoin(String, String),
    NotifClientIsWriting(String, Chan),
}

struct Channel {
    name: String,
    sender: BroadcastSenderWithList<BroadcastMessage, String>,
}

struct Client {
    name: String,
    password: String,
    is_connected: bool,
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

async fn handle_writer(writer: &mut OwnedWriteHalf, rx: &mut Receiver<ResponsePlusKey>) {
    let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);

    while let Some((object_to_send, key)) = rx.recv().await {
        typed_writer.send(&object_to_send, key).await.unwrap();
    }
}

async fn read_client_database(filepath: &str) -> Result<Vec<Client>, Box<dyn std::error::Error>> {
    let file = File::open(filepath).await?;
    let reader = BufReader::new(file);
    let mut clients: Vec<Client> = Vec::new();

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {

        let client_info: Vec<&str> = line.split(' ').collect();

        clients.push(Client {
            name: client_info[0].to_string(),
            password: client_info[1].to_string(),
            is_connected: false,
        });
    }

    Ok(clients)
}

async fn write_client_to_database(
    filepath: &str,
    clients: &[Client],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new().write(true).open(filepath).await?;
    let mut data = String::new();

    for client in clients.iter() {
        let client_info = vec![
            String::from_str(&client.name).unwrap(),
            String::from_str(&client.password).unwrap(),
        ]
        .join(" ");

        data.push_str(&client_info);
        data += "\n";
    }

    file.write_all(data.as_bytes()).await?;

    Ok(())
}

async fn respond_to_client(
    writer_tx: &mut Sender<ResponsePlusKey>,
    object_to_send: Response,
    key: Key,
) {
    if writer_tx.send((object_to_send, key)).await.is_err() {
        println!("!! An error occured !! {}", line!());
    }
}

async fn server_database(rx: &mut Receiver<AskRessource>) {

    let mut clients: Vec<Client> = read_client_database("./client_database.txt").await.unwrap(); // Database of clients (connected and not connected)

    let mut channels: Vec<Channel> = Vec::new(); // Database
   

    loop {
        match rx.recv().await {
            Some(AskRessource::JoinServer(username, password, resp)) => {
                let index_client = clients.iter().position(|client| client.name == username);
                if let Some(index) = index_client {
                    let client = &mut clients[index];

                    if client.is_connected {
                        if let Err(e) = resp.send(false) {
                            println!("!! An error occured in send: {e} !! {}", line!());
                        }
                    } else if client.password == password {
                        if let Err(e) = resp.send(true) {
                            println!("!! An error occured in send: {e} !! {}", line!());
                        }
                        client.is_connected = true;
                    } else if let Err(e) = resp.send(false) {
                        println!("!! An error occured in send: {e} !! {}", line!());
                    }
                } else {
                    if let Err(e) = resp.send(true) {
                        println!("!! An error occured in send: {e} !! {}", line!());
                    }

                    let client = Client {
                        name: username,
                        password,
                        is_connected: true,
                    };
                    clients.push(client);
                }

                write_client_to_database("./client_database.txt", &clients)
                    .await
                    .unwrap();
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
                    println!("!! An error occured !! {}", line!());
                } else {
                    channels.push(new_channel);
                }
            }

            Some(AskRessource::AskToJoinChannel(channel_name, username, resp)) => {
                match check_channel_exists(&channels, channel_name.clone()) {
                    Some(index) => {
                        let receiver = channels[index].sender.subscribe(username.clone());

                        if resp.send(receiver).is_err() {
                            println!("!! An error occured !! {}", line!());
                        }

                        // Informer les autres utilisateurs
                        let ressource =
                            BroadcastMessage::NewUserJoin(username.clone(), channel_name);
                        if channels[index].sender.send(ressource).is_err() {
                            println!("!! An error occured !! {}", line!());
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
                            println!("!! An error occured !! {}", line!());
                        }
                    }
                }
            }

            Some(AskRessource::UserDisconnected(username)) => {
                /* Remove user for connected clients array */
                let index = clients
                    .iter()
                    .position(|client| client.name == username)
                    .unwrap();

                clients[index].is_connected = false;

                /* Remove User from channels && remove channel if there is no user*/
                for channel_index in 0..channels.len() {
                    let private_channel_name = format!("{username}-privet-channel");
                    if check_user_in_channel(&channels, channel_index, username.clone()).is_some()
                        || channels[channel_index].name == private_channel_name
                    {
                        let ressource = BroadcastMessage::Leave(
                            username.clone(),
                            channels[channel_index].name.clone(),
                            true,
                        );
                        if channels[channel_index].sender.send(ressource).is_err() {
                            println!("!! An error occured !!\n  {}", line!());
                        }

                        if channels[channel_index].sender.into_subscribers().is_empty() {
                            channels.swap_remove(channel_index);
                        }
                    }
                }

                // As the user left, we delete his privet channel
                let index =
                    check_channel_exists(&channels, format!("{username}-privet-channel")).unwrap();
                channels.swap_remove(index);
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
                            println!("!! An error occured !!\n  {}", line!());
                        }
                    }
                }

                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n {}", line!());
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

                    let ressource = BroadcastMessage::DirectMessage(sender_name, message);

                    if channels[index].sender.send(ressource).is_err() {
                        println!("!! An error occured !!\n {}", line!());
                    }
                }

                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n {}", line!());
                }
            }

            Some(AskRessource::NotifClientIsWriting(username, channel)) => match &channel {
                Chan::Private(interlocutor_name) => {
                    let ressource =
                        BroadcastMessage::NotifClientIsWriting(username, channel.clone());

                    let index = check_channel_exists(
                        &channels,
                        format!("{interlocutor_name}-privet-channel"),
                    )
                    .unwrap();
                    if channels[index].sender.send(ressource).is_err() {
                        println!("!! An error occured !!\n {}", line!());
                    }
                }

                Chan::Public(channel_name) => {
                    let ressource =
                        BroadcastMessage::NotifClientIsWriting(username, channel.clone());

                    let index = check_channel_exists(&channels, channel_name.clone()).unwrap();
                    if channels[index].sender.send(ressource).is_err() {
                        println!("!! An error occured !!\n {}", line!());
                    }
                }
            },

            Some(AskRessource::AskToLeaveChannel(username, channel_name, resp)) => {
                let mut found = false;
                let channel_index = check_channel_exists(&channels, channel_name.clone());

                if let Some(index) = channel_index {
                    if check_user_in_channel(&channels, index, username.clone()).is_some() {
                        found = true;

                        let ressource = BroadcastMessage::Leave(username, channel_name, false);

                        if channels[index].sender.send(ressource).is_err() {
                            println!("!! An error occured !!\n {}", line!());
                        }

                        if channels[index].sender.into_subscribers().is_empty() {
                            channels.swap_remove(index);
                        }
                    }
                }

                if resp.send(found).is_err() {
                    println!("!! An error occured !!\n {}", line!());
                }
            }
            _ => {
                println!("!! An error occured !! {}", line!());
            }
        }
    }
}

async fn handle_waiting_for_messages(
    username: String,
    broadcast_receiver: &mut BroadcastReceiverWithList<BroadcastMessage, String>,
    writer_tx: &mut Sender<ResponsePlusKey>,
    key: Key,
) {
    let mut finished = false;
    while !finished {
        match broadcast_receiver.recv().await {
            Ok(BroadcastMessage::MessageToChannel(user_name, chan, content)) => {
                let op = ChanOp::Message {
                    from: user_name,
                    content,
                };

                respond_to_client(writer_tx, Response::Channel { op, chan }, key).await;
            }

            Ok(BroadcastMessage::DirectMessage(sender_name, message)) => {
                respond_to_client(
                    writer_tx,
                    Response::DirectMessage {
                        from: sender_name,
                        content: message,
                    },
                    key,
                )
                .await;
            }

            Ok(BroadcastMessage::NotifClientIsWriting(user_name, chan)) => {
                if user_name != username {
                    respond_to_client(
                        writer_tx,
                        Response::NotifClientIsWriting(user_name, chan),
                        key,
                    )
                    .await;
                }
            }

            Ok(BroadcastMessage::Leave(user_name, channel, suddenly_interrupt)) => {
                /* Current user wants to leave */

                if user_name == username {
                    finished = true;

                    if !suddenly_interrupt {
                        respond_to_client(writer_tx, Response::AckLeave(channel), key).await;
                    }
                } else {
                    let chan_op = ChanOp::UserDel(user_name);
                    respond_to_client(
                        writer_tx,
                        Response::Channel {
                            op: chan_op,
                            chan: channel,
                        },
                        key,
                    )
                    .await;
                }
            }
            Ok(BroadcastMessage::NewUserJoin(user_name, chan)) => {
                if username != user_name {
                    let op = ChanOp::UserAdd(user_name);
                    respond_to_client(writer_tx, Response::Channel { op, chan }, key).await;
                }
            }
            _ => {}
        }
    }
}

async fn handle_client(
    username: String,
    reader: &mut OwnedReadHalf,
    writer_tx: &mut Sender<ResponsePlusKey>,
    thread_tx: &mut Sender<AskRessource>,
    key: Key,
) {
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    loop {
        match typed_reader.recv(key).await {
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
                                key,
                            )
                            .await;

                            let mut copy_writer_tx = writer_tx.clone();
                            let copy_username = username.clone();

                            tokio::spawn(async move {
                                handle_waiting_for_messages(
                                    copy_username,
                                    &mut broadcast_receiver,
                                    &mut copy_writer_tx,
                                    key,
                                )
                                .await;
                                drop(broadcast_receiver);
                            });
                        }
                        Err(e) => {
                            println!("!! An error occured {e} !! {}", line!());
                        }
                    },
                    _ => {
                        println!("!! An error occured !! {}", line!());
                    }
                }
            }

            Ok(Some(Request::Message { to, content })) => {
                match to {
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
                                        Response::Error(ErrorType::Informative(
                                            "You must ask to join to channel OR channel not found"
                                                .to_string(),
                                        )),
                                        key,
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
                                    println!("!! An error occured in send !! {}", line!());
                                }
                            },
                            Err(e) => {
                                println!("!! An error occured in send: {e} !! {}", line!());
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
                            Ok(_) => {
                                match rx.await {
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
                                    Response::Error(ErrorType::DirectMessageReceiverNotFoundOrLeftTheServer(receiver_name)),
                                    key,
                                )
                                .await;
                                    }
                                    Err(e) => {
                                        println!("!! An error occured in send: {e} !! {}", line!());
                                    }
                                }
                            }

                            Err(e) => {
                                println!("!! An error occured in send: {e} !! {}", line!());
                            }
                        }
                    }
                }
            }
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
                                Response::Error(ErrorType::Informative(
                                    "You must ask to join to channel OR channel not found"
                                        .to_string(),
                                )),
                                key,
                            )
                            .await;
                        }
                    },
                    Err(_) => {
                        println!("!! An error occured in send: !! {}", line!());
                    }
                }
            }

            Ok(Some(Request::NotifClientIsWriting(username, channel))) => {
                let ressource =
                    AskRessource::NotifClientIsWriting(username.clone(), channel.clone());

                match thread_tx.send(ressource).await {
                    Ok(_) => {}
                    Err(_) => {
                        println!("!! An error occured in send: !! {}", line!());
                    }
                }
            }

            Err(_) => {
                let ressource = AskRessource::UserDisconnected(username.clone());
                match thread_tx.send(ressource).await {
                    Ok(_) => {
                        println!("-- {} left the server -- \n", username);
                    }
                    _ => {
                        println!("!! An error occured !! {}", line!());
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

async fn diffie_hellman_succeeded(
    server_public: &[u8; 32],
    reader: &mut OwnedReadHalf,
    writer_tx: &mut Sender<ResponsePlusKey>,
) -> Option<[u8; 32]> {
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    match typed_reader.recv(None).await {
        Ok(Some(Request::Handshake(client_public))) => {
            respond_to_client(writer_tx, Response::Handshake(*server_public), None).await;

            Some(client_public)
        }

        Ok(_) => {
            respond_to_client(
                writer_tx,
                Response::Error(ErrorType::Informative(
                    "We must firstly exchange keys".to_string(),
                )),
                None,
            )
            .await;
            None
        }
        _ => None,
    }
}

async fn is_client_accepted(
    reader: &mut OwnedReadHalf,
    writer_tx: &mut Sender<ResponsePlusKey>,
    thread_tx: &mut Sender<AskRessource>,
    key: Key,
) -> Option<String> {
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    match typed_reader.recv(key).await {
        Ok(Some(connection)) => {
            if let Request::Connect(username, password) = connection {
                let (tx, rx) = oneshot::channel();
                let ressource = AskRessource::JoinServer(username.clone(), password, tx);

                match thread_tx.send(ressource).await {
                    Ok(_) => match rx.await {
                        Ok(response) => {
                            if response {
                                println!("-- Connection from {} accepted --\n", username);

                                /* Respond to client that he is accepted on the server */
                                respond_to_client(
                                    writer_tx,
                                    Response::AckConnect(username.clone()),
                                    key,
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
                                                    key,
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
                                    Response::Error(ErrorType::Informative(
                                        "Another user with the same name already exist OR you have a connected session! OR your password is invalid"
                                            .to_string(),
                                    )),
                                    key,
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            println!("!! An error occured in receiving : {e} !!  {}", line!());
                        }
                    },
                    Err(e) => {
                        println!("!! An error occured in send: {e} !! {}", line!());
                    }
                }
            } else {
                println!("-- One user do not respect the protocol : quicked out --\n");
                respond_to_client(
                    writer_tx,
                    Response::Error(ErrorType::Informative(
                        "You must respect the protocol".to_string(),
                    )),
                    key,
                )
                .await;
            }
        }

        Ok(None) => {
            println!("!! An error occured in decryption mode !!, {}", line!());
        }

        _ => {
            println!("!! An error occured: !!, {}", line!());
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

        let (tx_writer, mut rx_writer): (Sender<ResponsePlusKey>, Receiver<ResponsePlusKey>) =
            mpsc::channel(100);

        // Diffie hellman keys
        let server_secret = EphemeralSecret::new(OsRng);
        let server_public = PublicKey::from(&server_secret);

        tokio::spawn(async move {
            handle_writer(&mut writer, &mut rx_writer).await;
        });

        tokio::spawn(async move {
            if let Some(client_public) = diffie_hellman_succeeded(
                server_public.as_bytes(),
                &mut reader,
                &mut tx_writer.clone(),                
            )
            .await
            {
                let shared_key = server_secret
                    .diffie_hellman(&PublicKey::from(client_public))
                    .to_bytes();
                let shared_key = Some(shared_key);

                if let Some(username) = is_client_accepted(
                    &mut reader,
                    &mut tx_writer.clone(),
                    &mut thread_tx,
                    shared_key,
                )
                .await
                {
                    handle_client(
                        username,
                        &mut reader,
                        &mut tx_writer.clone(),
                        &mut thread_tx,
                        shared_key,
                    )
                    .await;
                }
            }
        });
    }
}
