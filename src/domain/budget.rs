use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

impl Usage {
    pub fn tokens(&self) -> u64 {
        self.input.saturating_add(self.output)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Budget {
    pub limit: u64,
    pub used: u64,
    pub uncertain: u64,
    pub reservations: BTreeMap<String, u64>,
}

impl Budget {
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            used: 0,
            uncertain: 0,
            reservations: BTreeMap::new(),
        }
    }
    pub fn reserve(&mut self, tokens: u64) -> anyhow::Result<String> {
        anyhow::ensure!(
            tokens > 0 && tokens <= self.remaining(),
            "token budget exhausted"
        );
        let id = uuid::Uuid::new_v4().to_string();
        self.reservations.insert(id.clone(), tokens);
        Ok(id)
    }
    pub fn settle(&mut self, id: &str, usage: Option<&Usage>) -> anyhow::Result<()> {
        let reserved = self
            .reservations
            .remove(id)
            .ok_or_else(|| anyhow::anyhow!("unknown reservation"))?;
        if let Some(usage) = usage {
            self.used = self.used.saturating_add(usage.tokens());
        } else {
            // Lost streams can still be billable. Never reclaim unknown spend as free credit.
            self.uncertain = self.uncertain.saturating_add(reserved);
        }
        Ok(())
    }
    pub fn remaining(&self) -> u64 {
        self.limit
            .saturating_sub(self.used)
            .saturating_sub(self.uncertain)
            .saturating_sub(
                self.reservations
                    .values()
                    .copied()
                    .fold(0_u64, u64::saturating_add),
            )
    }
}
