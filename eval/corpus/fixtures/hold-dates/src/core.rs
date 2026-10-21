use serde_json::{Value,json};
pub fn solve(data:&Value)->Value {
 let s=data.as_str().unwrap(); let parts:Vec<_>=s.split('-').collect();
 if parts.len()!=3 || parts[0].len()!=4 || parts[1].len()!=2 || parts[2].len()!=2 {return Value::Null;}
 let nums:Option<Vec<u32>>=parts.iter().map(|p|p.parse().ok()).collect();let Some(n)=nums else{return Value::Null;};
 let (y,m,d)=(n[0],n[1],n[2]);if y==0 || !(1..=12).contains(&m){return Value::Null;}
 let days=[31,if y%4==0{29}else{28},31,30,31,30,31,31,30,31,30,31];
 if d==0 || d>days[(m-1)as usize]{Value::Null}else{json!(s)}
}
