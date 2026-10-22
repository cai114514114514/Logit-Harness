use crate::{
    config::{Config, Paths},
    runtime::state::Session,
};
use serde_json::{Value, json};

pub fn doctor(paths: &Paths, config: &Config) -> anyhow::Result<Value> {
    Ok(json!({"model":config.model,"base_url":config.base_url,
        "api_key_present":crate::config::api::configured(paths,&config.base_url)?,
        "api_key_source":crate::config::api::source(paths,&config.base_url)?,
        "chrome_present":config.chrome.is_file(),"service_sandbox_present":std::path::Path::new("/usr/bin/sandbox-exec").is_file(),
        "paid_probe_performed":false,"state_directory":paths.home,"platform":std::env::consts::OS,
        "version":env!("CARGO_PKG_VERSION"),"source_hash":env!("LOH_SOURCE_HASH")}))
}

pub(super) fn context(session: &Session) -> Value {
    let actors: Vec<_> = session.actors.values().map(|actor| json!({"id":actor.id,"items":actor.context.items.len(),"bytes":serde_json::to_vec(&actor.context.items).map(|v|v.len()).unwrap_or(0),"pending_tool_pairs":actor.context.unanswered().len()})).collect();
    json!({"measurement":"serialized context bytes; not provider tokens","actors":actors,"requirements":session.goal.requirements,"queue":session.queue,"user_turns":session.turns.len(),"mode":session.mode})
}

pub(super) fn usage(session: &Session) -> Value {
    json!({"reported":session.budget.reported,"kv_hit_ratio":session.budget.reported.cache_hit(),"accounted_tokens":session.budget.used,"uncertain_tokens":session.budget.uncertain,"reserved_tokens":session.budget.reservations,"remaining_tokens":session.budget.remaining(),"elapsed_seconds":session.elapsed,"cost":"not inferred from context bytes; campaign ledger remains authoritative"})
}

pub(super) fn permissions(session: Option<&Session>) -> Value {
    let pending: Vec<_> = session.into_iter().flat_map(|s|s.actions.values()).filter_map(|action| action.approval.as_ref().map(|id|json!({"approval":id,"action":action.id,"actor":action.actor,"tool":action.pending.call.name,"arguments":action.pending.call.arguments}))).collect();
    json!({"policy":"read-only scopes reject writes; external/unknown effects and policy-gated actions require exact-action approval","authorize":"/approve <ID>","rule_editor":false,"pending":pending})
}

pub(super) fn tasks(session: &Session, filter: &str) -> Value {
    let actors: Vec<_> = session
        .actors
        .values()
        .filter(|a| filter.is_empty() || a.id.contains(filter))
        .map(|a| json!({"id":a.id,"role":a.role,"phase":a.phase,"task":a.task,"retired":a.retired}))
        .collect();
    let actions: Vec<_> = session
        .actions
        .values()
        .filter(|a| filter.is_empty() || a.id.contains(filter) || a.actor.contains(filter))
        .collect();
    json!({"actors":actors,"actions":actions,"queue":session.queue,"owner":session.owner,"control":"/pause or /stop; this listing is observational"})
}
