mod core;
use std::io::{self, BufRead};
use serde_json::{Value, json};

fn main() {
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let answer = if request["op"] == "about" { json!({"name": "hold-glob", "version": 1}) } else { core::solve(&request["data"]) };
        println!("{}", answer);
    }
}
