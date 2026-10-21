use serde_json::{Value,json};
pub fn solve(data:&Value)->Value {
 let mut stock=data["stock"].as_i64().unwrap();let mut out=Vec::new();
 for batch in data["batches"].as_array().unwrap(){let mut ok=true;for item in batch.as_array().unwrap(){let n=item.as_i64().unwrap();if n>stock{ok=false;break;}stock-=n;}out.push(if ok{json!(stock)}else{json!("rejected")});}json!(out)
}
