use serde_json::{Value,json};
pub fn solve(data:&Value)->Value {
 let a:Vec<_>=data[0].as_str().unwrap().split('.').collect();let b:Vec<_>=data[1].as_str().unwrap().split('.').collect();
 json!(match a.cmp(&b){std::cmp::Ordering::Less=>-1,std::cmp::Ordering::Equal=>0,std::cmp::Ordering::Greater=>1})
}
