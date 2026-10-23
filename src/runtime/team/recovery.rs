//! Interrupted controls retain their inputs and block uncertain engineering completion.
use super::{MemberControl, ReceiptStatus, Target};
use crate::{
    domain::EventData,
    runtime::{
        actor::Phase,
        conversation::Boundary,
        state::{Question, Session},
    },
    store::Store,
};

pub(crate) fn recover_requests(session: &mut Session, store: &Store) -> anyhow::Result<()> {
    let uncertain = session
        .team
        .receipts
        .values()
        .filter(|r| r.applying)
        .map(|r| r.request.clone())
        .collect::<Vec<_>>();
    for request in uncertain {
        let mail_id = request.mail_id();
        let pending_input = session.team.receipts[&request.key()].pending_input.clone();
        let boundary_turn = session
            .boundary
            .as_ref()
            .and_then(|boundary| match boundary {
                Boundary::Plan { turn, .. } => turn.as_deref(),
                Boundary::Goal { turn, .. }
                | Boundary::Implement { turn, .. }
                | Boundary::ResumeGoal { turn, .. }
                | Boundary::Conversation { turn, .. } => Some(turn.as_str()),
            });
        if boundary_turn
            .is_some_and(|turn| turn == request.key() || Some(turn) == pending_input.as_deref())
        {
            // Boundary settlement is executable authority. Quarantine only the
            // deferred turn belonging to this uncertain request; other queued
            // Skills and passive workflow provenance have no matching turn.
            session.boundary = None;
        }
        let notice = "runner restarted while applying this request; it may have partially applied. Pending input is quarantined. Inspect current state and confirm intended requirements before retrying; the original input is preserved";
        for mail in session.team.mail.iter_mut().filter(|m| m.id == mail_id) {
            mail.error = Some(notice.into());
        }
        for input in session
            .inbox
            .iter_mut()
            .filter(|m| Some(&m.id) == pending_input.as_ref())
        {
            input.applied = true;
        }
        for actor in session.actors.values_mut().filter(|a| {
            a.turn == mail_id || a.turn == request.key() || Some(&a.turn) == pending_input.as_ref()
        }) {
            // Never replay an uncertain classification or side question. Recovery
            // below still settles actual process journals and provider accounting.
            actor.retired = true;
            actor.phase = Phase::Done;
        }
        let mut requirements = match &request.target {
            Target::Main => session.goal.active_versions(),
            Target::Member {
                assignment: Some(reference),
                ..
            } => session
                .team
                .assignments
                .get(&reference.id)
                .and_then(|a| session.actors.get(&a.actor))
                .map(|a| a.scope.requirements.clone())
                .unwrap_or_default(),
            _ => Default::default(),
        };
        requirements.retain(|id, _| session.goal.requirements.get(id).is_some_and(|r| r.active));
        let work_changing = matches!(
            request.action,
            MemberControl::Message { .. } | MemberControl::Skill { .. }
        );
        let mut error = notice.to_owned();
        if work_changing && !requirements.is_empty() {
            let id = request.recovery_id();
            let text = format!(
                "A request to {} has an uncertain partial outcome. Inspect existing effects; confirm the intended requirements using /answer {id} TEXT, or resubmit the intended input. Original request: {}",
                request.target.member_id(),
                serde_json::to_string(&request)?
            );
            session.questions.insert(
                id.clone(),
                Question {
                    target: Some(request.target.clone()),
                    text: text.clone(),
                    requirements,
                    answer: None,
                },
            );
            error.push_str(&format!("; unresolved review: /answer {id} TEXT"));
            if let Some(main) = session.actors.get_mut("main") {
                main.context.user(&text);
            }
            session.completion = None;
            session.verification_pending = false;
            session.final_report = None;
        }
        let pending = session.team.mail.iter().any(|m| {
            m.user
                && m.target.member_id() == request.target.member_id()
                && !m.delivered
                && m.error.is_none()
        });
        if let Some(member) = session.team.members.get_mut(request.target.member_id()) {
            member.interrupted = pending;
        }
        let receipt = session.team.receipts.get_mut(&request.key()).unwrap();
        receipt.applying = false;
        receipt.status = ReceiptStatus::Failed;
        receipt.error = Some(error);
        // Mail and receipt snapshots commit together; a failed request cannot
        // continue executing invisibly through an earlier queued side effect.
        store.commit(
            &session.id,
            session,
            EventData::RequestReceipt {
                goal: session.goal.id.clone(),
                receipt: session.team.receipts[&request.key()].clone(),
            },
        )?;
        for mail in session.team.mail.iter().filter(|m| m.id == mail_id) {
            store.append(
                &session.id,
                EventData::Mail {
                    goal: session.goal.id.clone(),
                    mail: mail.clone(),
                },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
