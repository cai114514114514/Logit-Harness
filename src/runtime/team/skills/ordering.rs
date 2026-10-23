//! Upgrade old pending receipts using durable arrival evidence, never ID order.
use super::*;
use crate::{domain::EventData, runtime::state::Control};
use std::collections::BTreeSet;

impl Runner {
    pub(super) fn restore_skill_queue(&mut self) -> anyhow::Result<()> {
        let pending = self
            .session
            .team
            .receipts
            .values()
            .filter(|r| {
                r.status == ReceiptStatus::Pending
                    && r.request.target != Target::Main
                    && matches!(r.request.action, MemberControl::Skill { .. })
            })
            .map(|r| r.request.key())
            .collect::<BTreeSet<_>>();
        let missing = pending
            .iter()
            .filter(|key| !self.session.team.skill_queue.contains(key))
            .cloned()
            .collect::<BTreeSet<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        let mut restored = vec![];
        // A receipt is published before handler execution, so first occurrence
        // records server arrival across both IPC and direct controls.
        for event in self.store.events(&self.session.id, 0)? {
            if let EventData::RequestReceipt { receipt, .. } = event.data {
                let key = receipt.request.key();
                if pending.contains(&key) && !restored.contains(&key) {
                    restored.push(key);
                }
            }
        }
        if restored.len() != pending.len() {
            // Use one complete ordering source. Event and control sequence
            // numbers are different domains and must never be mixed.
            restored.clear();
            for (_, value) in self.store.queued(&self.session.id, 0)? {
                if let Ok(Control::Directed { request }) = serde_json::from_value(value) {
                    let key = request.key();
                    if pending.contains(&key) && !restored.contains(&key) {
                        restored.push(key);
                    }
                }
            }
        }
        if restored.len() == pending.len() {
            self.session.team.skill_queue = restored;
        } else {
            for key in missing {
                let receipt = self.session.team.receipts.get_mut(&key).unwrap();
                receipt.status = ReceiptStatus::Failed;
                receipt.error = Some("saved Skill has no complete durable arrival-order evidence; input retained, review and resubmit".into());
            }
        }
        self.publish_metadata()
    }
}
