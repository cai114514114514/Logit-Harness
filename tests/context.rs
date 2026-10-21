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
