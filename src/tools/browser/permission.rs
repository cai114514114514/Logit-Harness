use crate::tools::{Scope, ToolError, approval_id};
use serde_json::{Value, json};

/// Approval binds the concrete action and its current origin. A redirect cannot
/// carry an approval for one website to a different website or port.
pub struct Permission<'a> {
    pub name: &'a str,
    pub arguments: &'a Value,
    pub scope: &'a Scope,
}

impl Permission<'_> {
    pub fn check(&self, location: &str) -> anyhow::Result<()> {
        let url = reqwest::Url::parse(location)?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https"),
            "browser interaction requires an HTTP(S) origin"
        );
        if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) {
            return Ok(());
        }
        anyhow::ensure!(
            !self.scope.readonly,
            "readonly actors cannot interact with remote state"
        );
        let origin = url.origin().ascii_serialization();
        let id = approval_id(
            self.name,
            &json!({"arguments":self.arguments,"origin":origin}),
        );
        if !self.scope.approvals.contains(&id) {
            return Err(ToolError::Approval {
                id,
                reason: format!("{} at {origin} may modify external state", self.name),
            }
            .into());
        }
        Ok(())
    }
}
