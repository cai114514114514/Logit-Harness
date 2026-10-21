use crate::tools::Effect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub transport: Transport,
    /// User declarations are authoritative; server-provided annotations are only hints.
    #[serde(default)]
    pub effects: BTreeMap<String, Effect>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Names of environment variables, never secret values in the configuration.
        #[serde(default)]
        environment: Vec<String>,
    },
    Http {
        url: String,
        /// Header name -> environment variable containing the full header value.
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

impl Server {
    pub fn effect(&self, tool: &str) -> Effect {
        let effect = self.effects.get(tool).copied().unwrap_or(Effect::Unknown);
        match (&self.transport, effect) {
            // A remote write cannot be cancelled by killing an owned local process.
            (Transport::Http { .. }, Effect::Write) => Effect::External,
            (_, Effect::Read | Effect::Write | Effect::External) => effect,
            _ => Effect::Unknown,
        }
    }
}
