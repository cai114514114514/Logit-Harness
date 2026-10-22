//! Controls are applied only by the session owner. Clients never rewrite saved
//! actor state while the daemon may still be settling a model/tool response.
use super::{
    conversation::{Boundary, Mode},
    runner::Runner,
};
use crate::{domain::EventData, extensions::mcp::Operation};

impl Runner {
    pub(super) fn settle_skills(&mut self) -> anyhow::Result<()> {
        while self.session.boundary.is_none() && !self.session.skill_pending.is_empty() {
            let request = self.session.skill_pending.remove(0);
            if let Err(error) = self.invoke_skill(&request.id, &request.hash, &request.arguments) {
                self.emit(EventData::Diagnostic {
                    message: format!("skill invocation failed: {error}"),
                })?;
            }
        }
        Ok(())
    }
    pub(super) fn invoke_skill(
        &mut self,
        id: &str,
        hash: &str,
        arguments: &str,
    ) -> anyhow::Result<()> {
        let knowledge = self
            .tools
            .knowledge
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("skills unavailable in this runner"))?;
        knowledge.refresh()?;
        let invocation = knowledge
            .skills
            .read()
            .map_err(|_| anyhow::anyhow!("skill catalog lock poisoned"))?
            .invoke(id, arguments)?;
        anyhow::ensure!(
            invocation.hash == hash,
            "skill changed since selection; invoke it again after reviewing /skills"
        );
        let text = invocation.request();
        let readonly = self.session.mode == Mode::Plan
            || matches!(self.session.boundary, Some(Boundary::Plan { .. }))
            || invocation.name == "review"
            || (invocation.name == "init"
                && std::fs::symlink_metadata(self.session.workspace.join("AGENTS.md")).is_ok());
        let turn = self.record_turn(&text);
        let initialize = invocation.id == "bundled:init";
        let description = format!(
            "capability changed: skill {} [{}] hash={} turn={turn}; arguments are literal data; permissions and budget unchanged",
            invocation.id, invocation.source, invocation.hash
        );
        // Attach provenance before any helper commits a User/Status event. A
        // crash can never leave a runnable workflow turn without its selected body.
        self.session.workflows.insert(turn.clone(), invocation);
        if readonly {
            self.side_turn(text, turn)?;
        } else if self.session.active_goal().is_some() {
            self.work_turn(text, turn)?;
        } else if initialize {
            // The user explicitly invoked initialization; the Markdown body is
            // guidance, not the authority for acquiring engineering ownership.
            self.request_boundary(Boundary::Goal { text, turn })?;
        } else {
            self.conversation_turn(text, turn)?;
        }
        self.emit(EventData::Diagnostic {
            message: description,
        })
    }

    pub(super) async fn settle_mcp(&mut self) -> anyhow::Result<()> {
        if !self.jobs.is_empty() || self.session.mcp_pending.is_empty() {
            return Ok(());
        }
        while let Some(change) = self.session.mcp_pending.first().cloned() {
            let result = match &self.tools.mcp {
                Some(registry) => registry.control(&change.server, change.operation).await,
                None => Err(anyhow::anyhow!("MCP unavailable in this runner")),
            };
            let message = match result {
                Ok(value) => {
                    match change.operation {
                        Operation::Disable => {
                            self.session.mcp_disabled.insert(change.server.clone());
                        }
                        Operation::Enable => {
                            self.session.mcp_disabled.remove(&change.server);
                        }
                        Operation::Reconnect => (),
                    }
                    let message = format!(
                        "capability changed: MCP {value}; pending mutation recovery is unchanged"
                    );
                    message
                }
                Err(error) => format!("MCP control failed for {}: {error}", change.server),
            };
            // Keep later changes in each committed snapshot. A crash can replay
            // this idempotent control, but cannot silently drop the rest of the queue.
            self.session.mcp_pending.remove(0);
            self.emit(EventData::Diagnostic { message })?;
        }
        Ok(())
    }
}
