use loh::model::Context;
use serde_json::json;

#[test]
fn interruptions_follow_the_entire_tool_batch_on_the_provider_wire() {
    let mut context = Context::default();
    context.user("original objective");
    context.append(&[
        json!({"type":"function_call","call_id":"one","name":"read_file","arguments":"{}"}),
        json!({"type":"function_call","call_id":"two","name":"check","arguments":"{}"}),
    ]);
    context.user("Use SQLite instead");
    context.tool_result("one", &json!({"observed":"JSON"}));
    context.user("Preserve the CLI");
    context.tool_result("two", &json!({"cancelled":true}));
    let wire = context.request_items().unwrap();
    assert_eq!(wire[3]["call_id"], "one");
    assert_eq!(wire[4]["call_id"], "two");
    assert_eq!(wire[5]["content"], "Use SQLite instead");
    assert_eq!(wire[6]["content"], "Preserve the CLI");
    assert_eq!(
        context.items[3]["content"], "Use SQLite instead",
        "saved chronology remains intact"
    );
    let saved: Context = serde_json::from_value(serde_json::to_value(&context).unwrap()).unwrap();
    assert_eq!(saved.request_items().unwrap(), wire);
}

#[test]
fn incomplete_or_orphan_tool_history_is_rejected_before_dispatch() {
    let mut context = Context::default();
    context
        .append(&[json!({"type":"function_call","call_id":"one","name":"check","arguments":"{}"})]);
    assert!(context.request_items().is_err());
    context.tool_result("one", &json!({"cancelled":true}));
    assert!(context.request_items().is_ok());
    context.tool_result("one", &json!({"duplicate":true}));
    assert!(context.request_items().is_err());
}

#[test]
fn state_updates_preserve_history_and_explicitly_replace_empty_sections() {
    let mut context = Context::default();
    context.user("Build a CLI");
    let initial = json!({"requirements":{"r1":"JSON"},"guidance":"old guidance","questions":{"q":"storage?"}});
    assert!(context.sync_state(initial.clone()).unwrap());
    let prefix = context.items.clone();
    let mut restored: Context =
        serde_json::from_value(serde_json::to_value(&context).unwrap()).unwrap();
    assert!(
        !restored.sync_state(initial).unwrap(),
        "reconnect must not duplicate state"
    );
    let current = json!({"requirements":{"r1":"SQLite"},"questions":{}});
    assert!(restored.sync_state(current.clone()).unwrap());
    assert!(restored.items.starts_with(&prefix));
    let update: serde_json::Value = serde_json::from_str(
        restored.items.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .strip_prefix("Logit- Harness state\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(update["kind"], "update");
    assert_eq!(update["set"]["requirements"]["r1"], "SQLite");
    assert_eq!(update["set"]["questions"], json!({}));
    assert_eq!(update["remove"], json!(["guidance"]));
    restored.compact("summary may be incomplete").unwrap();
    assert!(restored.sync_state(current.clone()).unwrap());
    let snapshot: serde_json::Value = serde_json::from_str(
        restored.items[1]["content"]
            .as_str()
            .unwrap()
            .strip_prefix("Logit- Harness state\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot["kind"], "snapshot");
    assert_eq!(
        snapshot["set"], current,
        "compaction must restore exact current requirements"
    );
}

#[test]
fn state_cannot_split_a_tool_batch_or_change_history_on_failure() {
    let mut context = Context::default();
    context.append(&[
        json!({"type":"function_call","call_id":"one","name":"read_file","arguments":"{}"}),
    ]);
    let before = serde_json::to_value(&context).unwrap();
    assert!(
        context
            .sync_state(json!({"requirements":{"r1":"SQLite"}}))
            .is_err()
    );
    assert_eq!(before, serde_json::to_value(&context).unwrap());
    context.user("Use SQLite");
    context.tool_result("one", &json!({"cancelled":true}));
    context
        .sync_state(json!({"requirements":{"r1":"SQLite"}}))
        .unwrap();
    let wire = context.request_items().unwrap();
    assert_eq!(wire[1]["type"], "function_call_output");
    assert_eq!(wire[2]["content"], "Use SQLite");
    assert_eq!(wire[3]["role"], "developer");
    assert!(context.sync_state(json!(null)).is_err());
}

fn oversized_history() -> Context {
    let mut context = Context::default();
    context.user("帮我审查一下代码");
    context.append(&[
        json!({"type":"function_call","call_id":"diff","name":"project_diff","arguments":"{}"}),
    ]);
    // The shape of the 2026-10-26 incident: thousands of fingerprints in one result.
    let files = (0..9197)
        .map(|i| (format!("artifacts/run/{i}.txt"), json!(format!("{i:064x}"))))
        .collect::<serde_json::Map<_, _>>();
    context.tool_result("diff", &json!({"result":{"files":files}}));
    context.user("继续");
    context
}

#[test]
fn a_summary_request_omits_oversized_items_but_keeps_tool_pairing() {
    let context = oversized_history();
    let full = serde_json::to_vec(&context.request_items().unwrap())
        .unwrap()
        .len();
    assert!(
        full > 512 * 1024,
        "fixture must exceed the total limit: {full}"
    );
    let items = context.compaction_items(64 * 1024, 512 * 1024).unwrap();
    let size = serde_json::to_vec(&items).unwrap().len();
    assert!(size < 4 * 1024, "{size} bytes");
    assert_eq!(items.len(), 4, "no item is dropped");
    assert_eq!(
        items[2]["call_id"], "diff",
        "the result still answers its call"
    );
    assert!(
        items[2]["output"]
            .as_str()
            .unwrap()
            .starts_with("[omitted ")
    );
    assert_eq!(items[0]["content"], "帮我审查一下代码");
    assert_eq!(items[3]["content"], "继续");
    // The saved history itself is untouched; only the request is bounded.
    assert_eq!(context.items.len(), 4);
    assert!(context.items[2]["output"].as_str().unwrap().len() > 512 * 1024);
}

#[test]
fn a_summary_request_also_fits_when_no_single_item_is_oversized() {
    let mut context = Context::default();
    for i in 0..40 {
        context.user(&format!("{i}:{}", "x".repeat(20 * 1024)));
    }
    let items = context.compaction_items(64 * 1024, 256 * 1024).unwrap();
    let size = serde_json::to_vec(&items).unwrap().len();
    assert!(size <= 256 * 1024, "{size} bytes");
    assert_eq!(items.len(), 40);
}
