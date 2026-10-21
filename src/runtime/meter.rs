use super::money::{Money, Prices};
use crate::model::{ModelEvent, Provider, Request, Response};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub use super::money::BudgetReached;

pub struct Metered {
    inner: Arc<dyn Provider>,
    money: Arc<Money>,
    prices: Prices,
}
impl Metered {
    pub fn new(inner: Arc<dyn Provider>, money: Arc<Money>, prices: Prices) -> Self {
        Self {
            inner,
            money,
            prices,
        }
    }
}
#[async_trait::async_trait]
impl Provider for Metered {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        // Reserve Flash's full documented input ceiling, including possible image tokens.
        // Actual usage releases the unused allowance after the response arrives.
        let input = 1_048_576;
        let maximum = self.prices.reserve(input, u64::from(request.max_output));
        let reservation = self.money.admit(maximum, &cancel).await?;
        let result = self.inner.respond(request, cancel, events).await;
        reservation.settle(
            result
                .as_ref()
                .ok()
                .map(|response| self.prices.cost(&response.usage))
                .or_else(|| {
                    result
                        .as_ref()
                        .err()?
                        .downcast_ref::<crate::model::ReportedFailure>()
                        .map(|failure| self.prices.cost(&failure.usage))
                }),
        )?;
        result
    }
}
