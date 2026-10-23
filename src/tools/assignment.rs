//! Versioned member write authority, rechecked under the host mutation lock.
use super::{Scope, ToolError, ToolHost};
use crate::model::ToolCall;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentLease {
    pub id: String,
    pub version: u64,
    pub actor: String,
}

#[derive(Clone, Debug)]
pub struct WriteGrant {
    pub lease: AssignmentLease,
    pub writable: BTreeSet<String>,
}

impl ToolHost {
    pub(super) fn check_assignment(
        &self,
        call: &ToolCall,
        scope: &Scope,
        grants: &BTreeMap<String, WriteGrant>,
    ) -> anyhow::Result<()> {
        let lease = scope.assignment.as_ref().expect("assignment checked");
        let grant = grants
            .get(&lease.id)
            .filter(|g| g.lease == *lease)
            .ok_or_else(|| ToolError::Stale("assignment owner or version changed".into()))?;
        anyhow::ensure!(
            call.name == "apply_patch",
            "member writes require a guarded patch"
        );
        let edits = call.arguments["edits"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("patch needs edits"))?;
        for edit in edits {
            let path = edit["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("edit path missing"))?;
            anyhow::ensure!(
                !std::path::Path::new(path).is_absolute()
                    && std::path::Path::new(path)
                        .components()
                        .all(|p| matches!(p, std::path::Component::Normal(_))),
                "invalid assignment path"
            );
            anyhow::ensure!(
                grant.writable.iter().any(|allowed| path == allowed
                    || (allowed.ends_with('/') && path.starts_with(allowed))),
                "path {path} is outside current assignment"
            );
            self.workspace.resolve(path)?;
        }
        Ok(())
    }
}
