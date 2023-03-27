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

use tokio::sync::Mutex;
use std::sync::Arc;

// let count = Arc::new(Mutex::new(0));

// let mut shared_data = Arc::new(Mutex::new(Vec::new()));



// let mut connected_clients: Vec<String> = Vec::new();

// Arc mutex mpsc channel

// async fn handle_connection(socket: TcpStream, connected_clients:<Arc<Mutex<Vec<>>>>) {}

// hashmap

#[tokio::main]

async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hello, world!");
    let shared_data:Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let listener = TcpListener::bind("127.0.0.1:8080").await?;
   
   loop {
    let (socket, _) = listener.accept().await?;
    let connected_clients = Arc::clone(&shared_data);

    tokio::spawn(async move {
        //handle_connection(socket, connected_clients).await
    
        let (reader, writer) = socket.into_split();
    let mut typed_reader = AsyncTypedReader::<_, Request>::new(reader);

    loop {
        let connection_request: Request = typed_reader.recv().await.unwrap().unwrap();
        let mut lock = connected_clients.lock();

        if let Request::Connect(username) = connection_request {
            print!("-- Connection from {} ", username);
            

            if (*lock.iter().find(|&s| s == &username).is_some()) {
                println!("refused --\n");
            } else {
                println!("accepted --\n");
                *lock.push(username.clone());

                let stream_response = TcpStream::connect("127.0.0.1:8080").await.unwrap();
                let (reader, writer) = stream_response.into_split();
                let mut typed_writer = AsyncTypedWriter::<_, Response>::new(writer);
                typed_writer
                    .send(&Response::AckConnect(username))
                    .await
                    .unwrap();

            }
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
