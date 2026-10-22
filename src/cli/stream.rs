use crate::{
    config::Paths,
    domain::{EventData, Status},
    runtime::{
        ipc::{self, ClientMessage, ServerMessage},
        state::Control,
    },
};
use std::io::{IsTerminal, Write};

pub async fn attach(paths: &Paths, id: &str, after: u64, json: bool) -> anyhow::Result<()> {
    if !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return super::terminal::attach(paths, id, after).await;
    }
    let socket = paths.socket(id)?;
    let mut reader = ipc::subscribe(&socket, after).await?;
    loop {
        let message = tokio::select! {
            result=ipc::receive(&mut reader)=>result?,
            _=tokio::signal::ctrl_c()=>{
                let _=ipc::request(&socket,ClientMessage::Command{command:Control::Pause}).await;return Ok(());
            }
        };
        if json {
            println!("{}", serde_json::to_string(&message)?);
        } else {
            match &message {
                ServerMessage::Snapshot { state, .. } => println!(
                    "Logit- Harness {} · {:?}\n{}",
                    state.id, state.status, state.title
                ),
                ServerMessage::Event { event } => match &event.data {
                    EventData::Message { actor, text } => println!("\n{actor}: {text}"),
                    EventData::ActionStarted { tool, .. } => println!("› {tool}"),
                    EventData::Status { status, reason } => println!("{status:?}: {reason}"),
                    EventData::Diagnostic { message } => println!("{message}"),
                    EventData::Approval { id, reason, .. } => {
                        println!("authorization required {id}: {reason}")
                    }
                    _ => (),
                },
                ServerMessage::Error { message } => eprintln!("{message}"),
                _ => (),
            }
        }
        std::io::stdout().flush()?;
        let status = match &message {
            ServerMessage::Snapshot { state, .. } => Some(&state.status),
            ServerMessage::Event { event } => {
                if let EventData::Status { status, .. } = &event.data {
                    Some(status)
                } else {
                    None
                }
            }
            _ => None,
        };
        if status.is_some_and(|s| {
            matches!(
                s,
                Status::Completed
                    | Status::Stopped
                    | Status::Exhausted
                    | Status::Failed
                    | Status::Waiting
            )
        }) {
            return Ok(());
        }
    }
}
