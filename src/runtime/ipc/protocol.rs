//! Live wire format is versioned separately from durable event serde.
use crate::{
    domain::SessionEvent,
    runtime::{
        state::{Control, Session},
        team::Request,
        view::{TargetView, ViewTarget},
    },
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
pub const PROTOCOL: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { protocol: u32 },
    Addressed { request: Request },
    SubscribeView { after: u64, target: ViewTarget },
    TargetStatus { target: ViewTarget },
    Receipt { client_id: String, id: String },
    Subscribe { after: u64 },
    Command { command: Control },
    Snapshot,
    Status,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello {
        protocol: u32,
    },
    Accepted {
        client_id: String,
        id: String,
        cursor: u64,
    },
    Receipt {
        receipt: Option<crate::runtime::team::Receipt>,
    },
    TargetView {
        state: Box<TargetView>,
        sequence: u64,
    },
    Event {
        event: SessionEvent,
    },
    Snapshot {
        state: Box<Session>,
        sequence: u64,
    },
    View {
        state: Box<crate::runtime::view::SessionView>,
        sequence: u64,
    },
    Ack,
    Error {
        message: String,
    },
}

pub(super) fn protocol_error() -> String {
    format!(
        "incompatible live protocol; this client requires protocol {PROTOCOL}. Keep the running daemon; detach and use its matching loh binary, or stop it explicitly before upgrading"
    )
}
pub(super) async fn read_message(
    reader: &mut BufReader<UnixStream>,
) -> anyhow::Result<ClientMessage> {
    let mut line = String::new();
    // Bound allocation while reading, including malicious clients with no newline.
    use tokio::io::AsyncReadExt;
    let count = reader.take(1024 * 1024 + 1).read_line(&mut line).await?;
    anyhow::ensure!(
        count > 0 && count <= 1024 * 1024,
        "control message empty or exceeds 1 MiB"
    );
    Ok(serde_json::from_str(&line)?)
}
pub(super) async fn send_client(
    stream: &mut UnixStream,
    message: &ClientMessage,
) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    Ok(())
}

pub(super) async fn send(stream: &mut UnixStream, message: &ServerMessage) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    Ok(())
}
