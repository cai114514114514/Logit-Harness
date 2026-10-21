use serde_json::{Value,json};
pub fn solve(data:&Value)->Value {
    let mut rows=data.as_array().unwrap().clone();
    rows.sort_by(|a,b| b["priority"].as_i64().cmp(&a["priority"].as_i64()).then(a["id"].as_str().cmp(&b["id"].as_str())));
    json!(rows.iter().map(|r|r["id"].clone()).collect::<Vec<_>>())
}
