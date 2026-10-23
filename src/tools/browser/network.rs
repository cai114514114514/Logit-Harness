//! Page request admission, not an OS network sandbox. DNS is screened locally;
//! Chrome resolves independently, so this does not promise DNS rebinding safety.
use crate::tools::address;
use chromiumoxide::{
    Page,
    cdp::browser_protocol::{
        fetch::{ContinueRequestParams, EventRequestPaused, FailRequestParams},
        network::{ErrorReason, SetBypassServiceWorkerParams},
    },
};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

pub async fn ensure_target(location: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(location)?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https"),
        "browser requires HTTP(S)"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("browser URL needs a host"))?;
    if let Some(ip) = address::literal(&url) {
        // Local development is explicitly authorized by the approved design.
        anyhow::ensure!(
            address::is_loopback(ip) || address::is_public(ip),
            "browser refuses a non-public address outside loopback development"
        );
        return Ok(());
    }
    if address::is_localhost(host) {
        return Ok(());
    }
    let addresses = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(80))),
    )
    .await??;
    let mut found = false;
    for address in addresses {
        found = true;
        // Only explicit loopback spellings receive the development exception.
        anyhow::ensure!(
            address::is_public(address.ip()),
            "browser hostname resolves to a non-public address"
        );
    }
    anyhow::ensure!(found, "browser hostname has no resolved addresses");
    Ok(())
}

pub async fn listen(
    page: &Page,
    output: Arc<Mutex<BTreeMap<String, Vec<Value>>>>,
    key: String,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let mut requests = page.event_listener::<EventRequestPaused>().await?;
    // Page requests must use the network admission path rather than a worker's
    // cached response. This does not isolate a worker's own networking.
    page.execute(SetBypassServiceWorkerParams::new(true))
        .await?;
    let page = page.clone();
    Ok(tokio::spawn(async move {
        while let Some(event) = requests.next().await {
            let decision = check_request(&event.request.url).await;
            let result = match decision {
                Ok(()) => page
                    .execute(ContinueRequestParams::new(event.request_id.clone()))
                    .await
                    .map(|_| ()),
                Err(error) => {
                    super::client::push(
                        &output,
                        &key,
                        json!({
                            "type":"blocked_request", "url":event.request.url, "reason":error.to_string()
                        }),
                    );
                    page.execute(FailRequestParams::new(
                        event.request_id.clone(),
                        ErrorReason::BlockedByClient,
                    ))
                    .await
                    .map(|_| ())
                }
            };
            if let Err(error) = result {
                super::client::push(
                    &output,
                    &key,
                    json!({
                        "type":"request_guard_error", "reason":error.to_string()
                    }),
                );
                // Never release a paused request after an admission/control failure.
                break;
            }
        }
    }))
}

async fn check_request(location: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(location)?;
    if matches!(url.scheme(), "data" | "blob" | "about") {
        // These carry inline/local page data and must not leave an event stalled.
        return Ok(());
    }
    ensure_target(location).await
}
