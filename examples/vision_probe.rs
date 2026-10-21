//! Explicit live transport probe. It always uses the configured shared campaign ledger.
use base64::Engine;
use loh::{
    config::Config,
    model::{DeepSeek, Provider, Request},
    runtime::{
        meter::Metered,
        money::{Money, RMB},
    },
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        arguments.len() == 3,
        "usage: vision_probe CAMPAIGN_HOME SCREENSHOT"
    );
    let home = PathBuf::from(&arguments[1]);
    let screenshot = PathBuf::from(&arguments[2]);
    let config = Config::load(&home)?;
    let campaign = config
        .campaign
        .ok_or_else(|| anyhow::anyhow!("a shared campaign budget is required"))?;
    anyhow::ensure!(
        campaign.limit <= 50 * RMB && config.model == "deepseek-flash",
        "probe must stay within the approved Flash campaign"
    );
    let bytes = std::fs::read(&screenshot)?;
    anyhow::ensure!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() <= 8 * 1024 * 1024,
        "expected a bounded PNG screenshot"
    );
    let money = Arc::new(Money::open(
        &home.join("money.sqlite"),
        &campaign.name,
        campaign.limit,
        &campaign.prices,
    )?);
    let provider = Metered::new(
        Arc::new(DeepSeek::new(
            config.base_url,
            config.model,
            std::env::var("DEEPSEEK_API_KEY")?,
        )?),
        money.clone(),
        campaign.prices,
    );
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let response = provider.respond(Request {
        instructions:"Read the supplied screenshot directly. Describe the visible input text, button label, and result text. Be concise and do not infer unseen behavior.".into(),
        input:vec![json!({"role":"user","content":[{"type":"input_text","text":"What does this actual browser screenshot show?"},{"type":"input_image","image_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(&bytes)),"detail":"high"}]})],
        tools:vec![], max_output:2048,
    }, tokio_util::sync::CancellationToken::new(), events).await?;
    let report = json!({"screenshot":screenshot,"hash":blake3::hash(&bytes).to_hex().to_string(),"model":response.model,"text":response.text(),"usage":response.usage,"campaign":money.balance()?});
    std::fs::write(
        home.join("vision-probe.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
