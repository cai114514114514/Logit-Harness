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
    attach_view(
        paths,
        id,
        after,
        json,
        crate::runtime::view::ViewTarget::main(),
    )
    .await
}
pub async fn attach_view(
    paths: &Paths,
    id: &str,
    after: u64,
    json: bool,
    target: crate::runtime::view::ViewTarget,
) -> anyhow::Result<()> {
    if target.is_main()
        && !json
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
    {
        return super::terminal::attach(paths, id, after).await;
    }
    let socket = paths.socket(id)?;
    let mut reader = ipc::subscribe_target(&socket, after, target.clone()).await?;
    loop {
        let message = tokio::select! {
            result=ipc::receive(&mut reader)=>result?,
            _=tokio::signal::ctrl_c()=>{
                let command = if target.is_main() { Control::Pause } else { Control::Directed { request: crate::runtime::team::Request { id: uuid::Uuid::new_v4().to_string(), client_id: "stream".into(), target: target.target.clone(), action: crate::runtime::team::MemberControl::Pause } } };
                let _=ipc::request(&socket,ClientMessage::Command{command}).await;return Ok(());
            }
        };
        if json {
            println!("{}", serde_json::to_string(&message)?);
        } else {
            match &message {
                ServerMessage::TargetView { state, .. } => println!(
                    "Logit- Harness {} · {} · {}",
                    state.session.id,
                    state.origin.target.member_id(),
                    if state.origin.readonly {
                        "只读归档"
                    } else {
                        "当前分配"
                    }
                ),
                ServerMessage::View { state, .. } => println!(
                    "Logit- Harness {} · {:?}\n{}",
                    state.id, state.status, state.title
                ),
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
            ServerMessage::TargetView { state, .. } => Some(&state.session.status),
            ServerMessage::View { state, .. } => Some(&state.status),
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
