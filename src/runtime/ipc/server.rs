//! One durable endpoint; each connection owns its replay cursor.
use super::{
    ClientMessage, PROTOCOL, ServerMessage,
    protocol::{protocol_error, read_message, send},
};
use crate::{
    domain::SessionEvent,
    runtime::{
        state::{Control, Session},
        view::ViewTarget,
    },
    store::Store,
};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    io::BufReader,
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;
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
    // Every live operation negotiates first. Historical event serde is separate.
    let hello = read_message(&mut reader).await?;
    if !matches!(hello, ClientMessage::Hello { protocol: PROTOCOL }) {
        send(
            reader.get_mut(),
            &ServerMessage::Error {
                message: protocol_error(),
            },
        )
        .await?;
        return Ok(());
    }
    send(
        reader.get_mut(),
        &ServerMessage::Hello { protocol: PROTOCOL },
    )
    .await?;
    let message = match read_message(&mut reader).await {
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
        ClientMessage::Hello { .. } => {
            send(
                reader.get_mut(),
                &ServerMessage::Error {
                    message: protocol_error(),
                },
            )
            .await?
        }
        ClientMessage::Addressed { request } => {
            let data = serde_json::to_value(Control::Directed {
                request: request.clone(),
            })?;
            match endpoint
                .store
                .enqueue_once(&endpoint.session, &request.key(), &data)
            {
                Ok(cursor) => {
                    let _ = endpoint.control.try_send(Control::Wake);
                    send(
                        reader.get_mut(),
                        &ServerMessage::Accepted {
                            client_id: request.client_id,
                            id: request.id,
                            cursor,
                        },
                    )
                    .await?;
                }
                Err(error) => {
                    send(
                        reader.get_mut(),
                        &ServerMessage::Error {
                            message: error.to_string(),
                        },
                    )
                    .await?
                }
            }
        }
        ClientMessage::Receipt { client_id, id } => {
            let (state, _): (Session, u64) = endpoint.store.snapshot(&endpoint.session)?;
            let key = serde_json::to_string(&[client_id, id])?;
            let receipt = state
                .team
                .receipts
                .values()
                .chain(
                    state
                        .archives
                        .iter()
                        .rev()
                        .flat_map(|a| a.team.receipts.values()),
                )
                .find(|r| r.request.key() == key)
                .cloned();
            send(reader.get_mut(), &ServerMessage::Receipt { receipt }).await?;
        }
        ClientMessage::TargetStatus { target } => {
            let (state, sequence): (Session, u64) = endpoint.store.snapshot(&endpoint.session)?;
            match state.target_view(&target) {
                Ok(state) => {
                    send(
                        reader.get_mut(),
                        &ServerMessage::TargetView {
                            state: Box::new(state),
                            sequence,
                        },
                    )
                    .await?
                }
                Err(error) => {
                    send(
                        reader.get_mut(),
                        &ServerMessage::Error {
                            message: error.to_string(),
                        },
                    )
                    .await?
                }
            }
        }
        ClientMessage::SubscribeView { after, target } => {
            subscribe_view(&mut reader, &endpoint, after, Some(target)).await?
        }
        ClientMessage::Command { command } => {
            // Ack means durable acceptance. A dropped client or runner cannot lose this input.
            let data = serde_json::to_value(&command)?;
            if let Control::Directed { request } = &command {
                if let Err(error) =
                    endpoint
                        .store
                        .enqueue_once(&endpoint.session, &request.key(), &data)
                {
                    send(
                        reader.get_mut(),
                        &ServerMessage::Error {
                            message: error.to_string(),
                        },
                    )
                    .await?;
                    return Ok(());
                }
            } else {
                endpoint.store.enqueue(&endpoint.session, &data)?;
            }
            let _ = endpoint.control.try_send(Control::Wake);
            send(reader.get_mut(), &ServerMessage::Ack).await?;
        }
        ClientMessage::Status => {
            let (state, sequence): (Session, u64) = endpoint.store.snapshot(&endpoint.session)?;
            send(
                reader.get_mut(),
                &ServerMessage::View {
                    state: Box::new(state.into()),
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
            subscribe_view(&mut reader, &endpoint, after, None).await?
        }
    }

    Ok(())
}

async fn subscribe_view(
    reader: &mut BufReader<UnixStream>,
    endpoint: &Endpoint,
    after: u64,
    target: Option<ViewTarget>,
) -> anyhow::Result<()> {
    let mut receiver = endpoint.events.subscribe();
    let (state, sequence): (Session, u64) = endpoint.store.snapshot(&endpoint.session)?;
    let message = if let Some(target) = target {
        match state.target_view(&target) {
            Ok(state) => ServerMessage::TargetView {
                state: Box::new(state),
                sequence,
            },
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
        }
    } else {
        ServerMessage::View {
            state: Box::new(state.into()),
            sequence,
        }
    };
    send(reader.get_mut(), &message).await?;
    // All events retain their single global cursor. Clients project locally;
    // filtering on the server would lose chronology and unknown actor bindings.
    let mut cursor = after;
    loop {
        for event in endpoint.store.events(&endpoint.session, cursor)? {
            cursor = event.sequence;
            send(reader.get_mut(), &ServerMessage::Event { event }).await?;
        }
        match receiver.recv().await {
            // A broadcast is only a wakeup. Recovery and other durable writers
            // may have committed earlier unbroadcast events; advancing directly
            // to this sequence would permanently skip them for this subscriber.
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => (),
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}
