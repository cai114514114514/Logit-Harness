use serde_json::{Value,json};
pub fn solve(data:&Value)->Value{let p=data["pattern"].as_str().unwrap().as_bytes();let t=data["text"].as_str().unwrap().as_bytes();json!(p.len()==t.len() && p.iter().zip(t).all(|(a,b)|*a==b'?' || a==b))}
