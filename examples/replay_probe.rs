//! Replay one saved request through the same metered provider, without executing its tools.
use loh::{
    config::Config,
    model::{Context, DeepSeek, Provider, Request},
    runtime::{
        meter::Metered,
        money::{Money, RMB},
    },
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: replay_probe CAMPAIGN_HOME SAVED_REQUEST OUTPUT_DIRECTORY"
    );
    let home = PathBuf::from(&args[1]);
    let source = PathBuf::from(&args[2]);
    let output = PathBuf::from(&args[3]);
    anyhow::ensure!(
        !output.exists(),
        "probe output already exists; inspect the original result before retrying"
    );
    let config = Config::load(&home)?;
    let campaign = config
        .campaign
        .ok_or_else(|| anyhow::anyhow!("shared campaign required"))?;
    anyhow::ensure!(
        campaign.limit <= 50 * RMB
            && config.model == "deepseek-flash"
            && config.base_url.trim_end_matches('/') == "https://api.deepseek.com",
        "probe must use the approved official Flash campaign"
    );
    let bytes = std::fs::read(&source)?;
    let raw: Value = serde_json::from_slice(&bytes)?;
    let mut context = Context::default();
    context.append(&serde_json::from_value::<Vec<Value>>(raw["input"].clone())?);
    let input = context.request_items()?;
    let request = Request {
        instructions: serde_json::from_value(raw["instructions"].clone())?,
        input,
        tools: serde_json::from_value(raw["tools"].clone())?,
        max_output: serde_json::from_value(raw["max_output"].clone())?,
    };
    let identity = format!("protocol-replay-{}", &blake3::hash(&bytes).to_hex()[..16]);
    let money = Arc::new(
        Money::open(
            &home.join("money.sqlite"),
            &campaign.name,
            campaign.limit,
            &campaign.prices,
        )?
        .for_session(&identity),
    );
    let provider = Metered::new(
        Arc::new(DeepSeek::new(
            config.base_url,
            config.model,
            std::env::var("DEEPSEEK_API_KEY")?,
        )?),
        money.clone(),
        campaign.prices,
    );
    std::fs::create_dir_all(&output)?;
    std::fs::write(
        output.join("request.json"),
        serde_json::to_vec_pretty(
            &json!({"source":source,"source_hash":blake3::hash(&bytes).to_hex().to_string(),"input":request.input,"instructions":request.instructions,"tools":request.tools,"max_output":request.max_output}),
        )?,
    )?;
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let response = provider
        .respond(request, tokio_util::sync::CancellationToken::new(), events)
        .await;
    let result = match &response {
        Ok(response) => {
            json!({"success":true,"model":response.model,"usage":response.usage,"output":response.output})
        }
        Err(error) => json!({"success":false,"error":error.to_string()}),
    };
    std::fs::write(
        output.join("response.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    let balance = money.session_balance(&identity)?;
    std::fs::write(
        output.join("cost.json"),
        serde_json::to_vec_pretty(&balance)?,
    )?;
    println!(
        "{}",
        json!({"output":output,"success":response.is_ok(),"cost":balance})
    );
    response?;
    Ok(())
}
