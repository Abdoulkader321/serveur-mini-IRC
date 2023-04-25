//! Ce crate contient plusieurs énumérations et structures utiles pour la communication entre
//! les clients mini-irc et le serveur mini-irc. Des communications via sockets "standards"
//! ou asynchrones (uniquement via [tokio]) sont supportés.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::Error;
use rand::Rng;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::io::{Read, Write};
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;
use tracing::info;

pub type Key = Option<[u8; 32]>;
pub type ResponsePlusKey = (Response, Key);

///  Une requête mini-irc, c'est-à-dire un message envoyé par le client au serveur.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum Request {
    Handshake([u8; 32]),
    /// Demande de connexion avec le nom d'utilisateur fourni.
    Connect(String),
    /// Demande de rejoindre un canal mini-irc donné. S'il n'existe pas encore, le canal est créé.
    JoinChan(String),
    /// Demande de quitter un canal mini-irc donné.
    LeaveChan(String),
    /// Message envoyé à un canal ou à un utilisateur.
    Message {
        to: MessageReceiver,
        content: String,
    },
}

/// La destinataire d'un message
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum MessageReceiver {
    User(String),
    Channel(String),
}

impl FromStr for MessageReceiver {
    // TODO: peut-être faire une vraie valeur d'erreur.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() < 2 {
            Err(format!(
                "Channel or username must be at least one character long: {s}"
            ))
        } else if let Some(s) = s.strip_prefix('#') {
            Ok(Self::Channel(s.to_string()))
        } else if let Some(s) = s.strip_prefix('@') {
            Ok(Self::User(s.to_string()))
        } else {
            Err(format!("Unrecognized receiver: {s}"))
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ChanOp {
    Message { from: String, content: String },
    UserAdd(String),
    UserDel(String),
}

/// Une réponse mini-irc, c'est-à-dire un message envoyé par le serveur au client.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum Response {
    Handshake([u8; 32]),
    /// Message direct d'un utilisateur.
    DirectMessage {
        from: String,
        content: String,
    },
    /// Message d'un channel (administratif ou utilisateur)
    Channel {
        op: ChanOp,
        chan: String,
    },
    /// Ack d'entrée dans un channel.
    AckJoin {
        chan: String,
        users: Vec<String>,
    },
    /// Ack de sortie d'un channel.
    AckLeave(String),
    /// Ack de connection, réponse indiquant que la demande a pu être correctement traitée.
    AckConnect(String),
    /// Message d'erreur
    Error(ErrorType),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum ErrorType {
    Informative(String),
    DirectMessageReceiverNotInTheServer(String),
    DirectMessageReceiverLeftTheServer(String)
}



/// Canal de communication côté réception, typé et **synchrone**. Permet de recevoir un type quelconque via
/// une socquette TCP par exemple, dès lors que le type à envoyer implémente [`Serialize`] et [`Deserialize`].
/// La socquette doit par ailleurs implémenter [`Read`].
///
/// # Exemple
///
/// ```no_run
/// use std::net::TcpStream;
/// use mini_irc_protocol::Response;
/// use mini_irc_protocol::TypedReader;
///
/// let stream = TcpStream::connect("serveur:port").unwrap();
/// let mut typed_reader = TypedReader::<_, Response>::new(stream);
/// let response: Response = typed_reader.recv().unwrap().unwrap();
/// ```
///
/// Ceci recevra une requête du serveur, qui aura été envoyée par le biais d'un [`AsyncTypedWriter`]
/// ou d'un [`TypedWriter`] pour le même type.

#[derive(Debug)]
pub struct TypedReader<Stream, T>
where
    Stream: Read,
{
    pub stream: Stream,
    _t: std::marker::PhantomData<*const T>,
}

unsafe impl<Stream, T> Send for TypedReader<Stream, T> where Stream: Send + Read {}

impl<Stream, T> TypedReader<Stream, T>
where
    Stream: Read,
{
    /// Créé un nouveau TypedReader
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            _t: std::marker::PhantomData,
        }
    }
}

impl<Stream, T> TypedReader<Stream, T>
where
    Stream: Read + std::fmt::Debug,
    T: DeserializeOwned + std::fmt::Debug,
{
    /// Reçoit un type via le canal de réception. Il doit avoir été envoyé via
    /// la fonction [`AsyncTypedWriter::send`] ou [`TypedWriter::send`].
    ///
    /// Renvoie une erreur en cas d'erreur du canal sous-jacent, et
    /// `None` en cas d'erreur de déserialisation.
    #[tracing::instrument(level = "debug")]
    pub fn recv(&mut self, supplied_key: Key) -> std::io::Result<Option<T>> {
        // Read the size, from u32
        info!("Receiving data");
        let mut size = [0; 4];
        self.stream.read_exact(&mut size)?;
        let size = u32::from_be_bytes(size);
        // Prepare a buffer
        let mut buf = vec![0; size as usize];
        self.stream.read_exact(&mut buf)?;

        if let Some(key) = supplied_key {
            //buf = decrypt(&buf, &key);
            match decrypt(&buf, &key) {
                Ok(plaintext) => {
                    info!("Data received");
                    // Deserialize the value, discard the potential deserializing error
                    return Ok(bincode::deserialize(&plaintext).ok());
                }
                Err(e) => {
                    info!("Data received but malformed");
                    return Ok(None);
                }
            }
        }

        Ok(bincode::deserialize(&buf).ok())
    }
}
/// Canal de communication côté émission, typé et **synchrone**. Permet d'envoyer un type quelconque via
/// une socquette TCP par exemple, dès lors que le type à envoyer implémente [`Serialize`] et [`Deserialize`].
/// La socquette doit par ailleurs implémenter [`Write`].
///
/// # Exemple
///
/// ```no_run
/// use std::net::TcpStream;
/// use mini_irc_protocol::Request;
/// use mini_irc_protocol::TypedWriter;
///
/// let stream = TcpStream::connect("serveur:port").unwrap();
/// let mut typed_writer = TypedWriter::<_, Request>::new(stream);
/// typed_writer.send(&Request::Connect("toto".to_string())).unwrap();
/// ```
///
/// Ceci enverra une requête au serveur, qui devra être reçue via un [`AsyncTypedReader`] ou
/// un [`TypedReader`] pour le même type.
#[derive(Debug)]
pub struct TypedWriter<Stream, T>
where
    Stream: Write,
{
    pub stream: Stream,
    _t: std::marker::PhantomData<*const T>,
}

unsafe impl<Stream, T> Send for TypedWriter<Stream, T> where Stream: Send + Write {}

impl<Stream, T> TypedWriter<Stream, T>
where
    Stream: Write,
{
    /// Créé un nouveau TypedReader
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            _t: std::marker::PhantomData,
        }
    }
}

impl<Stream, T> TypedWriter<Stream, T>
where
    Stream: Write + std::fmt::Debug,
    T: serde::Serialize + std::fmt::Debug,
{
    /// Envoie un type via le canal sélectionné. Une erreur est envoyée en cas
    /// d'erreur du canal sous-jacent.
    #[tracing::instrument(level = "info")]
    pub fn send(&mut self, value: &T, supplied_key: Key) -> std::io::Result<()> {
        let mut data: Vec<u8> = bincode::serialize(value).unwrap();

        if let Some(key) = supplied_key {
            data = encrypt(&data, &key);
        }

        // Send the size, as u32
        self.stream.write_all(&(data.len() as u32).to_be_bytes())?;
        self.stream.write_all(&data)
    }
}

/// Canal de communication côté réception, typé et **asynchrone**. Permet de recevoir un type quelconque via
/// une socquette TCP par exemple, dès lors que le type à envoyer implémente [`Serialize`] et [`Deserialize`].
/// La socquette doit par ailleurs implémenter [`AsyncReadExt`].
///
/// # Exemple
///
/// ```no_run
/// use tokio::net::TcpStream;
/// use mini_irc_protocol::Response;
/// use mini_irc_protocol::AsyncTypedReader;
///
/// # #[tokio::main]
/// # async fn main() {
/// let stream = TcpStream::connect("serveur:port").await.unwrap();
/// let (reader, writer) = stream.into_split();
/// let mut typed_reader = AsyncTypedReader::<_, Response>::new(reader);
/// let response: Response = typed_reader.recv().await.unwrap().unwrap();
/// # }
/// ```
///
/// Ceci recevra une requête du serveur, qui aura été envoyée par le biais d'un [`AsyncTypedWriter`]
/// ou d'un [`TypedWriter`] pour le même type.

#[derive(Debug)]
pub struct AsyncTypedReader<Stream, T>
where
    Stream: AsyncReadExt,
{
    pub stream: Stream,
    _t: std::marker::PhantomData<*const T>,
}

unsafe impl<Stream, T> Send for AsyncTypedReader<Stream, T> where Stream: Send + AsyncReadExt {}

impl<Stream, T> AsyncTypedReader<Stream, T>
where
    Stream: AsyncReadExt,
{
    /// Créé un nouveau AsyncTypedReader
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            _t: std::marker::PhantomData,
        }
    }
}
impl<Stream, T> AsyncTypedReader<Stream, T>
where
    Stream: AsyncReadExt + std::marker::Unpin + std::fmt::Debug,
    T: DeserializeOwned + std::fmt::Debug,
{
    /// Reçoit un type via le canal réception. Il doit avoir été envoyé via
    /// la fonction [`AsyncTypedWriter::send`] ou [`TypedWriter::send`].
    ///
    /// Renvoie une erreur en cas d'erreur du canal sous-jacent, et
    /// `None` en cas d'erreur de déserialisation.
    #[tracing::instrument(level = "debug")]
    pub async fn recv(&mut self, supplied_key: Key) -> std::io::Result<Option<T>> {
        // Read the size, from u32
        info!("Receiving data");
        let mut size = [0; 4];
        self.stream.read_exact(&mut size).await?;
        let size = u32::from_be_bytes(size);
        //info!("Received size");
        // Prepare a buffer
        let mut buf = vec![0; size as usize];
        self.stream.read_exact(&mut buf).await?;

        if let Some(key) = supplied_key {
            match decrypt(&buf, &key){
                Ok(plaintext) => {
                    info!("Data received: {:?}", plaintext);
                    return Ok(bincode::deserialize(&plaintext).ok());
                }

                Err(e) => {
                    info!("Received invalid data");
                    return Ok(None);
                }
            }
        }

        let data = bincode::deserialize(&buf).ok();

        match data.as_ref() {
            Some(data) => {
                info!("Data received: {:?}", data);
            }
            _ => {
                info!("Received invalid data");
            }
        }
        // Deserialize the value, discard the potential deserializing error
        Ok(data)
    }
}

/// Canal de communication côté émission, typé et **asynchrone**. Permet d'envoyer un type quelconque via
/// une socquette TCP par exemple, dès lors que le type à envoyer implémente [`Serialize`] et [`Deserialize`].
/// La socquette doit par ailleurs implémenter [`AsyncWriteExt`].
///
/// # Exemple
///
/// ```no_run
/// use tokio::net::TcpStream;
/// use mini_irc_protocol::Request;
/// use mini_irc_protocol::AsyncTypedWriter;
///
///
/// # #[tokio::main]
/// # async fn main() {
/// let stream = TcpStream::connect("serveur:port").await.unwrap();
/// let (reader, writer) = stream.into_split();
/// let mut typed_writer = AsyncTypedWriter::<_, Request>::new(writer);
/// typed_writer.send(&Request::Connect("toto".to_string())).await.unwrap();
/// # }
/// ```
///
/// Ceci enverra une requête au serveur, qui devra être reçue via un [`AsyncTypedReader`] ou
/// un [`TypedReader`] pour le même type.

#[derive(Debug)]
pub struct AsyncTypedWriter<Stream, T>
where
    Stream: AsyncWriteExt,
{
    pub stream: Stream,
    _t: std::marker::PhantomData<*const T>,
}

unsafe impl<Stream, T> Send for AsyncTypedWriter<Stream, T> where Stream: Send + AsyncWriteExt {}

impl<Stream, T> AsyncTypedWriter<Stream, T>
where
    Stream: AsyncWriteExt,
{
    /// Créé un nouveau AsyncTypedWriter
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            _t: std::marker::PhantomData,
        }
    }
}

impl<Stream, T> AsyncTypedWriter<Stream, T>
where
    Stream: AsyncWriteExt + std::marker::Unpin + std::fmt::Debug,
    T: serde::Serialize + std::fmt::Debug,
{
    /// Envoie un type via le canal sélectionné. Une erreur est envoyée en cas
    /// d'erreur du canal sous-jacent.
    #[tracing::instrument(level = "debug")]
    pub async fn send(&mut self, value: &T, supplied_key: Key) -> std::io::Result<()> {
        let mut data: Vec<u8> = bincode::serialize(value).unwrap();

        if let Some(key) = supplied_key {
            data = encrypt(&data, &key);
        }
        // Send the size, as u32
        self.stream
            .write_all(&(data.len() as u32).to_be_bytes())
            .await?;
        self.stream.write_all(&data).await
    }
}

pub struct BroadcastSenderWithList<T, U>
where
    T: Clone,
    U: 'static + PartialEq + Clone,
{
    sender: broadcast::Sender<T>,
    subscribers: Arc<Mutex<Vec<U>>>,
}

pub struct BroadcastReceiverWithList<T, U>
where
    T: Clone,
    U: 'static + PartialEq + Clone,
{
    receiver: broadcast::Receiver<T>,
    subscribers: Arc<Mutex<Vec<U>>>,
    identifier: U,
}

impl<T, U> Debug for BroadcastSenderWithList<T, U>
where
    T: Clone,
    U: PartialEq + 'static + Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastSenderWithList")
            .field("sender", &self.sender)
            .finish()
    }
}

impl<T, U> BroadcastSenderWithList<T, U>
where
    T: Clone,
    U: PartialEq + 'static + Clone,
{
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn subscribe(&mut self, identity: U) -> BroadcastReceiverWithList<T, U> {
        if self
            .subscribers
            .lock()
            .unwrap()
            .iter()
            .any(|v| v == &identity)
        {
            panic!("Identity already present in subscriber list");
        }

        self.subscribers.lock().unwrap().push(identity.clone());

        BroadcastReceiverWithList {
            receiver: self.sender.subscribe(),
            subscribers: self.subscribers.clone(),
            identifier: identity,
        }
    }

    pub fn send(&self, data: T) -> Result<usize, tokio::sync::broadcast::error::SendError<T>> {
        self.sender.send(data)
    }

    pub fn subscribers(&mut self) -> &std::sync::Mutex<Vec<U>> {
        self.subscribers.deref()
    }

    pub fn into_subscribers(&self) -> Vec<U> {
        self.subscribers.lock().unwrap().clone()
    }
}

impl<T, U> BroadcastReceiverWithList<T, U>
where
    T: Clone,
    U: PartialEq + 'static + Clone,
{
    pub async fn recv(&mut self) -> Result<T, tokio::sync::broadcast::error::RecvError> {
        self.receiver.recv().await
    }

    pub fn into_subscribers(&self) -> Vec<U> {
        self.subscribers.lock().unwrap().clone()
    }
}

impl<T, U> Debug for BroadcastReceiverWithList<T, U>
where
    T: Clone,
    U: PartialEq + 'static + Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastReceiverWithList")
            .field("receiver", &self.receiver)
            .finish()
    }
}

impl<T, U> Drop for BroadcastReceiverWithList<T, U>
where
    T: Clone,
    U: PartialEq + 'static + Clone,
{
    // We must remove the relevant receiver from list
    fn drop(&mut self) {
        self.subscribers
            .lock()
            .unwrap()
            .retain(|v| v != &self.identifier);
    }
}

fn encrypt(data_to_encrypt: &[u8], key: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).unwrap();

    let mut random_slice = [0_u8; 12];
    rand::rngs::OsRng.fill(&mut random_slice);

    let nonce = aes_gcm::Nonce::from_slice(&random_slice);
    let ciphertext = cipher.encrypt(nonce, data_to_encrypt.as_ref()).unwrap();

    [nonce.to_vec(), ciphertext].concat()
}

fn decrypt(data_to_decrypt: &[u8], key: &[u8]) -> Result<Vec<u8>, aes_gcm::Error> {
    let cipher = Aes256Gcm::new((key).into());

    //let plaintext = cipher.decrypt(data_to_decrypt[..12].into(), data_to_decrypt[12..].as_ref()).unwrap();

    cipher.decrypt(data_to_decrypt[..12].into(), data_to_decrypt[12..].as_ref())
}
