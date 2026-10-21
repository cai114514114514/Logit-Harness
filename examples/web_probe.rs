//! Two explicitly metered live web calls; returned pages are data, never instructions.
use loh::{
    config::Config,
    runtime::money::{Money, RMB},
    tools::web::{WebClient, WebUsage},
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        args.len() == 3,
        "usage: web_probe CAMPAIGN_HOME OUTPUT_DIRECTORY"
    );
    let home = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    anyhow::ensure!(
        !output.exists(),
        "inspect the saved probe before starting another"
    );
    let config = Config::load(&home)?;
    let campaign = config
        .campaign
        .ok_or_else(|| anyhow::anyhow!("shared campaign required"))?;
    anyhow::ensure!(
        campaign.limit <= 50 * RMB && config.base_url == "https://api.deepseek.com",
        "use the approved official campaign"
    );
    let money = Arc::new(
        Money::open(
            &home.join("money.sqlite"),
            &campaign.name,
            campaign.limit,
            &campaign.prices,
        )?
        .for_session("web-integration-probe"),
    );
    std::fs::create_dir_all(&output)?;
    let client = WebClient::new(
        config.base_url,
        std::env::var("DEEPSEEK_API_KEY")?,
        2,
        WebUsage::default(),
    )?
    .with_money(money.clone(), campaign.prices)
    .with_usage_file(output.join("quota.json"))?;
    let search = client
        .search(
            "DeepSeek Responses API official documentation",
            2,
            CancellationToken::new(),
        )
        .await?;
    std::fs::write(
        output.join("search.json"),
        serde_json::to_vec_pretty(&search)?,
    )?;
    let fetch = client
        .fetch(
            "https://api-docs.deepseek.com/guides/responses_api/",
            CancellationToken::new(),
        )
        .await?;
    std::fs::write(
        output.join("fetch.json"),
        serde_json::to_vec_pretty(&fetch)?,
    )?;
    let cost = money.session_balance("web-integration-probe")?;
    std::fs::write(output.join("cost.json"), serde_json::to_vec_pretty(&cost)?)?;
    anyhow::ensure!(
        search["results"]
            .as_array()
            .is_some_and(|results| !results.is_empty()),
        "search returned no evidence"
    );
    anyhow::ensure!(
        fetch["content"]
            .as_str()
            .is_some_and(|text| text.contains("Responses")),
        "fetch lacks the requested page content"
    );
    println!(
        "{}",
        json!({"output":output,"search_results":search["results"].as_array().map(Vec::len),"fetch_bytes":fetch["content"].as_str().map(str::len),"cost":cost})
    );
    Ok(())
}
