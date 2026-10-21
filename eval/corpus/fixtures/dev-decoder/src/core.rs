use serde_json::{Value,json};
pub fn solve(data:&Value)->Value {
 let text=data.as_str().unwrap(); let raw=text.as_bytes(); let mut out=Vec::new(); let mut i=0;
 while i<raw.len() { if raw[i]==b'%' && i+2<raw.len() { if let Ok(n)=u8::from_str_radix(&text[i+1..i+3],16) {out.push(n);i+=3;continue;} } out.push(if raw[i]==b'+' {b' '} else {raw[i]});i+=1; }
 match String::from_utf8(out) { Ok(s)=>json!(s),Err(_)=>json!({"error":"invalid"}) }
}
