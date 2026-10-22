//! Inspect the same tool specs used by dispatch, without launching transports,
//! discovering project guidance, probing Chrome or constructing a provider.
use crate::{
    config::{Config, Paths},
    runtime::state::Session,
    tools::{self, Effect, ToolSpec},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

pub(super) fn inspect(
    paths: &Paths,
    root: &Path,
    mut config: Config,
    session: Option<&Session>,
    filter: &str,
) -> anyhow::Result<Value> {
    if let Some(defaults) = session.and_then(|s| s.defaults.as_ref()) {
        defaults.apply(&mut config);
    }
    if let Some(spec) = session.and_then(|s| s.evaluation.as_ref()) {
        config.mcp.clear();
        config.user_skills = spec.protocol.user_skills;
        config.search_limit = spec.protocol.search_limit;
    }
    let workspace = tools::files::Workspace::open(root)?;
    let logs = paths.home.join("capability-inspection"); // Constructors do no I/O here.
    let host = tools::ToolHost::new(
        workspace.clone(),
        Arc::new(tokio::sync::RwLock::new(crate::domain::Goal::new(""))),
        logs.clone(),
    );
    let mut entries = vec![];
    let sandbox = Path::new("/usr/bin/sandbox-exec").is_file();
    for spec in host.specs() {
        let available = spec.name != "check" || sandbox;
        entries.push(entry(
            spec,
            "builtin",
            available,
            if available {
                "local implementation; concrete call still subject to runtime policy"
            } else {
                "macOS source-write sandbox missing"
            },
        ));
    }
    let knowledge = Arc::new(crate::extensions::knowledge::Knowledge::new(
        root.into(),
        vec![],
    )?);
    for tool in crate::extensions::skills::tools(knowledge) {
        entries.push(entry(
            tool.spec(),
            "skills",
            true,
            "metadata/load on demand; no Skill body grants authority",
        ));
    }
    let services = Arc::new(tools::services::Services::new(
        workspace,
        logs.join("services"),
    ));
    for tool in tools::services::tools(services) {
        let spec = tool.spec();
        let available = spec.name != "service_start" || sandbox;
        entries.push(entry(
            spec,
            "services",
            available,
            "session-owned services; start requires the macOS source-write sandbox",
        ));
    }
    let browser = Arc::new(tools::browser::BrowserClient::new(
        config.chrome.clone(),
        logs.join("browser"),
    ));
    for tool in tools::browser::tools(browser) {
        entries.push(entry(tool.spec(),"browser",config.chrome.is_file(),"Chrome path presence only, not a launch probe; concrete remote interactions may require scoped approval"));
    }
    let key = crate::config::api::configured(paths, &config.base_url)?;
    for spec in tools::web::specs() {
        entries.push(entry(spec,"web",key && config.search_limit>0,"credential/quota configuration only, not remote availability; calls consume quota and may incur metered cost"));
    }
    let servers=config.mcp.iter().map(|(name,server)|json!({"name":name,"enabled":session.is_none_or(|s|!s.mcp_disabled.contains(name)),"configured_effects":server.effects.keys().map(|name|(name,server.effect(name))).collect::<std::collections::BTreeMap<_,_>>(),"remote_tools":"not discovered; use /mcp for configuration and explicit reconnect"})).collect::<Vec<_>>();
    let enabled = servers.iter().any(|server| server["enabled"] == true);
    let configured = !servers.is_empty();
    let registry = Arc::new(crate::extensions::mcp::Registry::new(
        config.mcp,
        root.into(),
        logs.join("mcp"),
    ));
    for tool in crate::extensions::mcp::tools(registry) {
        let spec = tool.spec();
        let available = match spec.name.as_str() {
            "mcp_list" => true,
            "mcp_pending" | "mcp_resolve" => configured,
            _ => enabled,
        };
        entries.push(entry(spec,"mcp",available,"dispatcher only; concrete server/tool effect comes from configuration; unknown/external effects require exact-action approval; remote readiness unprobed"));
    }
    entries.retain(|entry| {
        filter.is_empty()
            || entry["name"]
                .as_str()
                .is_some_and(|name| name.contains(filter))
            || entry["source"].as_str() == Some(filter)
    });
    entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(
        json!({"tools":entries,"mcp_servers":servers,"remote_probe_performed":false,
        "availability":"configured local prerequisites only; not a live daemon/remote readiness or permission grant",
        "mode":session.map(|s|s.mode),"policy":"conversation/Plan accepts concrete Read effects only; engineering writes need workspace ownership and current scope; external/unknown and policy-gated actions require exact-action approval via /approve; model role also limits advertised tools",
        "runtime_controls":"goal, steering, worker and verification controls are role-scoped runtime tools, not general host capabilities",
        "history":"/history tools [query] opens prior tool calls"}),
    )
}
fn entry(spec: ToolSpec, source: &str, available: bool, condition: &str) -> Value {
    json!({"name":spec.name,"description":spec.description,"effect":spec.effect,"parameters":spec.parameters,"source":source,"available":available,"condition":condition,"conversation_or_plan":spec.effect==Effect::Read})
}
