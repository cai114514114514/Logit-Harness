//! Public ownership and mail updates precede the output they describe.
use super::{runner::Runner, state::Session, view::ActorBinding};
use crate::domain::EventData;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Public {
    seen: BTreeMap<String, serde_json::Value>,
}
impl Public {
    fn changed(&mut self, key: String, data: &EventData) -> anyhow::Result<bool> {
        let value = serde_json::to_value(data)?;
        if self.seen.get(&key) == Some(&value) {
            return Ok(false);
        }
        self.seen.insert(key, value);
        Ok(true)
    }
    pub fn updates(&mut self, session: &Session) -> anyhow::Result<Vec<EventData>> {
        let mut updates = vec![];
        // Current actors may share legacy main IDs with archived goals. Each
        // emission carries its goal, so replay never guesses from actor spelling.
        for ActorBinding {
            actor,
            target,
            goal,
        } in session.actor_bindings().into_values()
        {
            let key = format!("actor:{actor}");
            let data = EventData::ActorBound {
                actor,
                target,
                goal,
            };
            if self.changed(key, &data)? {
                updates.push(data);
            }
        }
        for (goal, team) in session
            .archives
            .iter()
            .map(|a| (&a.goal.id, &a.team))
            .chain(std::iter::once((&session.goal.id, &session.team)))
        {
            for mail in &team.mail {
                let data = EventData::Mail {
                    goal: goal.clone(),
                    mail: mail.clone(),
                };
                if self.changed(serde_json::to_string(&("mail", goal, &mail.id))?, &data)? {
                    updates.push(data);
                }
            }
            for receipt in team.receipts.values() {
                let receipt_goal = if !session
                    .team
                    .members
                    .contains_key(receipt.request.target.member_id())
                {
                    session
                        .archives
                        .iter()
                        .rev()
                        .find(|a| {
                            a.team
                                .members
                                .contains_key(receipt.request.target.member_id())
                        })
                        .map(|a| &a.goal.id)
                        .unwrap_or(goal)
                } else {
                    goal
                };
                let data = EventData::RequestReceipt {
                    goal: receipt_goal.clone(),
                    receipt: receipt.clone(),
                };
                if self.changed(
                    serde_json::to_string(&("receipt", receipt_goal, receipt.request.key()))?,
                    &data,
                )? {
                    updates.push(data);
                }
            }
        }
        Ok(updates)
    }
}
impl Runner {
    pub(super) fn publish_metadata(&self) -> anyhow::Result<()> {
        let updates = self
            .public
            .lock()
            .map_err(|_| anyhow::anyhow!("public event projection poisoned"))?
            .updates(&self.session)?;
        for data in updates {
            let event = self.store.commit(&self.session.id, &self.session, data)?;
            self.publish(event)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
