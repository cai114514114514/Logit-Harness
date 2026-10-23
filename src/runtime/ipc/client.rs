//! Every client path negotiates before controls, views or replay.
use super::{
    ClientMessage, PROTOCOL, ServerMessage,
    protocol::{protocol_error, send_client},
};
use crate::runtime::view::ViewTarget;
use std::path::Path;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
};
async fn connect(path: &Path) -> anyhow::Result<BufReader<UnixStream>> {
    let mut stream = UnixStream::connect(path).await?;
    send_client(&mut stream, &ClientMessage::Hello { protocol: PROTOCOL }).await?;
    let mut reader = BufReader::new(stream);
    let response =
        tokio::time::timeout(std::time::Duration::from_secs(5), receive(&mut reader)).await;
    match response {
        Ok(Ok(ServerMessage::Hello { protocol: PROTOCOL })) => (),
        Ok(Ok(ServerMessage::Hello { protocol })) => anyhow::bail!(
            "{}; daemon protocol {protocol}, client protocol {PROTOCOL}",
            protocol_error()
        ),
        Ok(Ok(ServerMessage::Error { message })) => {
            anyhow::bail!("{}; daemon: {message}", protocol_error())
        }
        _ => anyhow::bail!("{}", protocol_error()),
    }
    Ok(reader)
}
pub async fn request(path: &Path, message: ClientMessage) -> anyhow::Result<ServerMessage> {
    let mut reader = connect(path).await?;
    send_client(reader.get_mut(), &message).await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), receive(&mut reader)).await?
}
pub async fn subscribe(path: &Path, after: u64) -> anyhow::Result<BufReader<UnixStream>> {
    let mut reader = connect(path).await?;
    send_client(reader.get_mut(), &ClientMessage::Subscribe { after }).await?;
    Ok(reader)
}
pub async fn subscribe_target(
    path: &Path,
    after: u64,
    target: ViewTarget,
) -> anyhow::Result<BufReader<UnixStream>> {
    let mut reader = connect(path).await?;
    send_client(
        reader.get_mut(),
        &ClientMessage::SubscribeView { after, target },
    )
    .await?;
    Ok(reader)
}
pub async fn check_protocol(path: &Path) -> anyhow::Result<()> {
    connect(path).await.map(|_| ())
}

pub async fn receive(reader: &mut BufReader<UnixStream>) -> anyhow::Result<ServerMessage> {
    let mut line = String::new();
    anyhow::ensure!(
        reader.read_line(&mut line).await? > 0,
        "runner disconnected; reconnect to replay missed events"
    );
    Ok(serde_json::from_str(&line)?)
}
