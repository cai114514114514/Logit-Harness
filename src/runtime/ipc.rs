use super::state::{Control, Session};
use crate::{domain::SessionEvent, store::Store};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Subscribe { after: u64 },
    Command { command: Control },
    Snapshot,
    Status,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Event {
        event: SessionEvent,
    },
    Snapshot {
        state: Box<Session>,
        sequence: u64,
    },
    View {
        state: super::view::SessionView,
        sequence: u64,
    },
    Ack,
    Error {
        message: String,
    },
}

#[derive(Clone)]
struct Endpoint {
    session: String,
    store: Arc<Store>,
    control: mpsc::Sender<Control>,
    events: broadcast::Sender<SessionEvent>,
}
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    inode: u64,
    endpoint: Endpoint,
}
impl Server {
    pub async fn bind(
        path: PathBuf,
        session: String,
        store: Arc<Store>,
        control: mpsc::Sender<Control>,
        events: broadcast::Sender<SessionEvent>,
    ) -> anyhow::Result<Self> {
        if path.exists() {
            anyhow::ensure!(
                UnixStream::connect(&path).await.is_err(),
                "session runner is already connected"
            );
            let metadata = std::fs::symlink_metadata(&path)?;
            anyhow::ensure!(
                metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() },
                "refusing to replace an unrelated socket path"
            );
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let inode = std::fs::metadata(&path)?.ino();
        Ok(Self {
            listener,
            path,
            inode,
            endpoint: Endpoint {
                session,
                store,
                control,
                events,
            },
        })
    }
    pub async fn run(self, cancel: CancellationToken) -> anyhow::Result<()> {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _=cancel.cancelled()=>break,
                result=self.listener.accept()=>{let(stream,_)=result?;let endpoint=self.endpoint.clone();clients.spawn(async move{let _=serve(stream,endpoint).await;});}
                Some(_)=clients.join_next(),if !clients.is_empty()=>(),
            }
        }
        clients.abort_all();
        while clients.join_next().await.is_some() {}
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.ino() == self.inode && m.file_type().is_socket())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

async fn serve(stream: UnixStream, endpoint: Endpoint) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).await? == 0 {
        return Ok(());
    }
    anyhow::ensure!(line.len() <= 1024 * 1024, "control message exceeds 1 MiB");
    let message: ClientMessage = match serde_json::from_str(&line) {
        Ok(value) => value,
        Err(error) => {
            send(
                reader.get_mut(),
                &ServerMessage::Error {
                    message: error.to_string(),
                },
            )
            .await?;
            return Ok(());
        }
    };
    match message {
        ClientMessage::Command { command } => {
            // Ack means durable acceptance. A dropped client or runner cannot lose this input.
            endpoint
                .store
                .enqueue(&endpoint.session, &serde_json::to_value(command)?)?;
            let _ = endpoint.control.try_send(Control::Wake);
            send(reader.get_mut(), &ServerMessage::Ack).await?;
        }
        ClientMessage::Status => {
            let (state, sequence): (Session, u64) = endpoint.store.snapshot(&endpoint.session)?;
            send(
                reader.get_mut(),
                &ServerMessage::View {
                    state: state.into(),
                    sequence,
                },
            )
            .await?;
        }
        ClientMessage::Snapshot => {
            let (state, sequence) = endpoint.store.snapshot(&endpoint.session)?;
            send(
                reader.get_mut(),
                &ServerMessage::Snapshot { state, sequence },
            )
            .await?;
        }
        ClientMessage::Subscribe { after } => {
            let mut receiver = endpoint.events.subscribe();
            let (state, sequence) = endpoint.store.snapshot(&endpoint.session)?;
            send(
                reader.get_mut(),
                &ServerMessage::Snapshot { state, sequence },
            )
            .await?;
            let mut cursor = after;
            for event in endpoint.store.events(&endpoint.session, cursor)? {
                cursor = event.sequence;
                send(reader.get_mut(), &ServerMessage::Event { event }).await?;
            }
            loop {
                match receiver.recv().await {
                    Ok(event) if event.sequence > cursor => {
                        cursor = event.sequence;
                        send(reader.get_mut(), &ServerMessage::Event { event }).await?;
                    }
                    Ok(_) => (),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        for event in endpoint.store.events(&endpoint.session, cursor)? {
                            cursor = event.sequence;
                            send(reader.get_mut(), &ServerMessage::Event { event }).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    Ok(())
}

async fn send(stream: &mut UnixStream, message: &ServerMessage) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    Ok(())
}

pub async fn request(path: &Path, message: ClientMessage) -> anyhow::Result<ServerMessage> {
    let mut stream = UnixStream::connect(path).await?;
    let mut bytes = serde_json::to_vec(&message)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    let mut reader = BufReader::new(stream);
    tokio::time::timeout(std::time::Duration::from_secs(5), receive(&mut reader)).await?
}
pub async fn subscribe(path: &Path, after: u64) -> anyhow::Result<BufReader<UnixStream>> {
    let mut stream = UnixStream::connect(path).await?;
    let bytes = format!(
        "{}\n",
        serde_json::to_string(&ClientMessage::Subscribe { after })?
    );
    stream.write_all(bytes.as_bytes()).await?;
    Ok(BufReader::new(stream))
}
pub async fn receive(reader: &mut BufReader<UnixStream>) -> anyhow::Result<ServerMessage> {
    let mut line = String::new();
    anyhow::ensure!(
        reader.read_line(&mut line).await? > 0,
        "runner disconnected; reconnect to replay missed events"
    );
    Ok(serde_json::from_str(&line)?)
}
