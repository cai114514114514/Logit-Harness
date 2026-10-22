//! Recovery reconciles reality before permitting new work; it never replays a write intent.
use super::{actor::Phase, state::Session};
use crate::{
    domain::EventData,
    store::Store,
    tools::{files::Workspace, process::ProcessRecord},
};
use serde_json::json;
use std::path::Path;

pub async fn reconcile(session: &mut Session, store: &Store, logs: &Path) -> anyhow::Result<()> {
    let interrupted = session.actions.values().any(|a| a.running)
        || session
            .actors
            .values()
            .any(|a| a.busy > 0 || a.phase == Phase::Busy)
        || !session.budget.reservations.is_empty()
        || session.transition.is_some();
    if !interrupted {
        return Ok(());
    }

    // Scan the journal rather than only Action.pid: the daemon may die between
    // durable process creation and delivery of its ProcessStarted event.
    if logs.is_dir() {
        for entry in std::fs::read_dir(logs)? {
            let path = entry?.path();
            if !path.to_string_lossy().ends_with(".process.json") {
                continue;
            }
            let record: ProcessRecord = serde_json::from_slice(&std::fs::read(&path)?)?;
            if record.output.is_none() {
                record.stop().await?;
            }
        }
    }
    // A crashed chat model request does not justify discovering the project.
    let snapshot = if session.mode == super::conversation::Mode::Engineering
        || session
            .actions
            .values()
            .any(|a| a.running && !a.pending.scope.readonly)
    {
        Workspace::open(&session.workspace)?.fingerprints()?
    } else {
        Default::default()
    };
    let interrupted_ids: Vec<_> = session
        .actions
        .values()
        .filter(|a| a.running)
        .map(|a| a.id.clone())
        .collect();
    for id in interrupted_ids {
        let action = session.actions.remove(&id).unwrap();
        let actor = session
            .actors
            .get_mut(&action.actor)
            .ok_or_else(|| anyhow::anyhow!("interrupted actor missing"))?;
        actor.context.tool_result(&action.pending.call.id, &json!({
            "error":"Operation interrupted; not replayed. Inspect current files and process logs before deciding whether any action remains necessary.",
            "current_files":snapshot,"logs":logs,"action":id
        }));
        actor.scope.observed.clear();
        actor.phase = Phase::Ready;
    }
    for actor in session.actors.values_mut() {
        if actor.busy > 0 || actor.phase == Phase::Busy {
            actor.busy = 0;
            actor.phase = if actor.pending.is_empty() {
                Phase::Ready
            } else {
                Phase::Awaiting
            };
            actor.context.user("The runner restarted. Interrupted operations were reconciled, not replayed. Read current state before continuing; preserve concurrent user changes.");
        }
        for id in actor.context.unanswered() {
            let queued = actor.pending.iter().any(|p| p.call.id == id)
                || session
                    .actions
                    .values()
                    .any(|a| a.actor == actor.id && a.pending.call.id == id);
            if !queued {
                actor.context.tool_result(&id, &json!({"error":"Tool result was interrupted and is not replayed. Inspect current files before planning a replacement."}));
            }
        }
    }
    let reservations: Vec<_> = session.budget.reservations.keys().cloned().collect();
    for id in reservations {
        session.budget.settle(&id, None)?;
    }
    if let Some(target) = session.transition.take() {
        session.status = target;
    }
    store.commit(&session.id, session, EventData::Diagnostic { message: "Recovery reconciled interrupted processes and current files; uncertain operations were not replayed".into() })?;
    Ok(())
}
