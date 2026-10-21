//! Admission waits for concurrent reservations; an abandoned submitted call remains a liability.
use super::money::{BudgetReached, Money};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
#[error("request cancelled before budget admission")]
pub struct NotSubmitted;

pub struct Charge {
    money: Arc<Money>,
    id: Option<String>,
}
impl Charge {
    pub fn settle(mut self, amount: Option<u64>) -> anyhow::Result<()> {
        self.money.settle(self.id.as_ref().unwrap(), amount)?;
        self.id = None;
        Ok(())
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        if let Some(id) = &self.id {
            // On database failure the original reservation remains, still limiting spend.
            let _ = self.money.settle(id, None);
        }
    }
}
impl Money {
    pub async fn admit(
        self: &Arc<Self>,
        amount: u64,
        cancel: &CancellationToken,
    ) -> anyhow::Result<Charge> {
        loop {
            if cancel.is_cancelled() {
                return Err(NotSubmitted.into());
            }
            match self.reserve(amount) {
                Ok(id) => {
                    return Ok(Charge {
                        money: self.clone(),
                        id: Some(id),
                    });
                }
                Err(error) if error.is::<BudgetReached>() => {
                    let balance = self.balance()?;
                    if amount
                        > balance
                            .limit
                            .saturating_sub(balance.used)
                            .saturating_sub(balance.uncertain)
                    {
                        return Err(error);
                    }
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(NotSubmitted.into()),
                        _ = tokio::time::sleep(Duration::from_millis(50)) => (),
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}
