use super::Context;
use serde_json::{Map, Value, json};

impl Context {
    /// Append current harness state without changing a previously sent prefix.
    /// Each changed top-level section replaces that section in full; empty maps
    /// and removals are explicit, so stale requirements or approvals cannot linger.
    pub fn sync_state(&mut self, state: Value) -> anyhow::Result<bool> {
        let next = state
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("harness state must be an object"))?;
        // Never insert a new message between a tool batch and its outputs. This
        // also checks orphan/duplicate results before modifying durable history.
        self.request_items()?;
        let set: Map<_, _> = next
            .iter()
            .filter(|(key, value)| {
                self.state.as_ref().and_then(|old| old.get(*key)) != Some(*value)
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let remove: Vec<_> = self
            .state
            .as_ref()
            .into_iter()
            .flat_map(|old| old.keys())
            .filter(|key| !next.contains_key(*key))
            .cloned()
            .collect();
        if self.state.is_some() && set.is_empty() && remove.is_empty() {
            return Ok(false);
        }
        let update = json!({
            "kind": if self.state.is_none() { "snapshot" } else { "update" },
            "set": set,
            "remove": remove,
        });
        // DeepSeek accepts developer messages as user-role input. Keep changing
        // state here, not in `instructions` (the first system message). These
        // records report state; they never grant new user authorization.
        self.items.push(json!({
            "role": "developer",
            "content": format!("Logit- Harness state\n{update}"),
        }));
        self.state = Some(next.clone());
        Ok(true)
    }
}
