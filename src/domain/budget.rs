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
        // input 已包含缓存命中的 token，cached 是其子集，不能再次累加。
        // 此处计算 token 数量；缓存与非缓存的不同费率由 Money 单独结算。
        self.input.saturating_add(self.output)
    }
    pub fn accumulate(&mut self, usage: &Self) {
        self.input = self.input.saturating_add(usage.input);
        self.output = self.output.saturating_add(usage.output);
        self.cached = self.cached.saturating_add(usage.cached);
    }
    /// Weighted by input tokens, never by requests. Output is not cache-eligible
    /// input and must not dilute the denominator. No report is distinct from 0%.
    pub fn cache_hit(&self) -> Option<f64> {
        (self.input != 0).then(|| self.cached as f64 / self.input as f64)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Budget {
    pub limit: u64,
    pub used: u64,
    pub uncertain: u64,
    pub reservations: BTreeMap<String, u64>,
    /// Provider-reported totals. Older snapshots may only have `used`; their
    /// absent cache measurements must not be reconstructed from token estimates.
    #[serde(default)]
    pub reported: Usage,
}

impl Budget {
    /// limit 为 0 时 remaining 恒为 0，所有 reserve 都会失败——
    /// 零预算表示完全禁止模型调用，而不是"不设限"。
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            used: 0,
            uncertain: 0,
            reservations: BTreeMap::new(),
            reported: Usage::default(),
        }
    }
    /// 预留没有过期机制：reserve 之后永不 settle（例如进程崩溃且状态丢失）
    /// 会永久占住这部分额度，恢复依赖事件日志重放来兜底。
    pub fn reserve(&mut self, tokens: u64) -> anyhow::Result<String> {
        anyhow::ensure!(tokens > 0, "cannot reserve zero tokens");
        anyhow::ensure!(
            self.limit == super::limit::UNLIMITED || tokens <= self.remaining(),
            "token budget exhausted: requested {tokens}, remaining {}",
            self.remaining()
        );
        let id = uuid::Uuid::new_v4().to_string();
        self.reservations.insert(id.clone(), tokens);
        Ok(id)
    }
    /// 不校验 actual <= reserved：供应商实际用量可能超出预留估计，超出部分
    /// 照常计入 used（remaining 用 saturating_sub 兜底为 0），不因超支拒绝结算。
    pub fn settle(&mut self, id: &str, usage: Option<&Usage>) -> anyhow::Result<()> {
        let reserved = self
            .reservations
            .remove(id)
            .ok_or_else(|| anyhow::anyhow!("unknown reservation"))?;
        if let Some(usage) = usage {
            self.used = self.used.saturating_add(usage.tokens());
            self.reported.accumulate(usage);
        } else {
            // Lost streams can still be billable. Never reclaim unknown spend as free credit.
            self.uncertain = self.uncertain.saturating_add(reserved);
        }
        Ok(())
    }
    pub fn remaining(&self) -> u64 {
        if self.limit == super::limit::UNLIMITED {
            return super::limit::UNLIMITED;
        }
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
