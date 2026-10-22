use loh::model::Request;
use serde_json::{Map, Value};

/// Decode the same chronological state contract the model receives. Tests must
/// inspect the latest state, not match obsolete snapshots somewhere in history.
pub fn current(request: &Request) -> Value {
    let mut state = Map::new();
    for item in &request.input {
        if item["role"] != "developer" {
            continue;
        }
        let Some(text) = item["content"]
            .as_str()
            .and_then(|text| text.strip_prefix("Logit- Harness state\n"))
        else {
            continue;
        };
        let change: Value = serde_json::from_str(text).unwrap();
        if change["kind"] == "snapshot" {
            state.clear();
        }
        state.extend(change["set"].as_object().unwrap().clone());
        for key in change["remove"].as_array().unwrap() {
            state.remove(key.as_str().unwrap());
        }
    }
    Value::Object(state)
}
