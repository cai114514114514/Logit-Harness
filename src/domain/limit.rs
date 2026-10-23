//! One explicit unlimited value shared by token and time admission.
//! The integer representation keeps saved sessions and IPC numeric; configuration
//! and user-facing output spell it `unlimited`. Zero continues to mean no allowance.
use serde::{Deserialize, Deserializer, Serializer};

pub const UNLIMITED: u64 = u64::MAX;

pub fn parse(text: &str) -> anyhow::Result<u64> {
    if text.eq_ignore_ascii_case("unlimited") {
        return Ok(UNLIMITED);
    }
    let value = text.parse::<u64>()?;
    anyhow::ensure!(value > 0, "限额必须为正数或 unlimited");
    Ok(value)
}

pub fn label(value: u64) -> String {
    if value == UNLIMITED {
        "无限制".into()
    } else {
        value.to_string()
    }
}

pub fn value(limit: u64) -> serde_json::Value {
    if limit == UNLIMITED {
        serde_json::json!("unlimited")
    } else {
        limit.into()
    }
}

pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    if *value == UNLIMITED {
        serializer.serialize_str("unlimited")
    } else {
        serializer.serialize_u64(*value)
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Number(u64),
        Text(String),
    }
    match Input::deserialize(deserializer)? {
        Input::Number(value) => Ok(value),
        Input::Text(text) => parse(&text).map_err(serde::de::Error::custom),
    }
}
