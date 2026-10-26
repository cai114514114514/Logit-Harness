//! Tool output is data, never Markdown. Preserve command bytes and typed results.
use serde_json::Value;

fn data(result: &Value) -> &Value {
    result
        .get("result")
        .and_then(|value| value.get("data"))
        .or_else(|| result.get("data"))
        .unwrap_or(result)
}

/// Chinese verb for a tool. Unknown tools keep their own name.
pub(super) fn verb(tool: &str) -> &str {
    match tool {
        "read_file" => "读取",
        "search_files" | "search" | "search_text" | "list_files" => "搜索",
        "apply_patch" => "修改",
        "shell" => "运行",
        "check" => "检查",
        "web_fetch" | "fetch" => "抓取",
        tool if tool.starts_with("browser") => "浏览",
        tool => tool,
    }
}

/// Tools whose step is labelled with the clauses it serves. Reading and
/// searching serve every clause, so naming them is noise.
pub(super) fn serves_clauses(tool: &str) -> bool {
    matches!(tool, "apply_patch" | "shell" | "check")
}

/// The measured result shown at the right of a tool line, or nothing. Every
/// value comes from the typed result; nothing is estimated.
pub(super) fn outcome(result: &Value) -> Option<String> {
    let data = data(result);
    let process = &data["process"];
    if process["timed_out"] == true {
        return Some("超时".into());
    }
    if process["cancelled"] == true {
        return Some("已取消".into());
    }
    if let Some(code) = data["exit_code"]
        .as_i64()
        .or_else(|| process["exit_code"].as_i64())
        .or_else(|| process["code"].as_i64())
    {
        return if code != 0 {
            Some(format!("退出 {code}"))
        } else if data["source_stable"] == false {
            // A check that changed the sources it inspected does not count.
            Some("检查期间文件有变化".into())
        } else {
            passed_tests(&self::result(result)).map(|n| format!("{n} 通过"))
        };
    }
    if let Some(matches) = data["matches"].as_array() {
        return Some(format!("{} 个结果", matches.len()));
    }
    if let Some(content) = data["content"]
        .as_str()
        .filter(|_| data["path"].is_string())
    {
        return Some(format!("{} 行", content.lines().count()));
    }
    if let Some(changes) = data["changes"].as_array() {
        let mut added = 0;
        let mut removed = 0;
        for change in changes {
            added += change["added"].as_u64()?;
            removed += change["removed"].as_u64()?;
        }
        return Some(format!("+{added} −{removed}"));
    }
    None
}

/// Passed-test total from summary lines that test runners print. Only exact,
/// known summary shapes count; anything else reports nothing.
fn passed_tests(output: &str) -> Option<u64> {
    static SUMMARY: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let summary = SUMMARY.get_or_init(|| {
        regex::Regex::new(
            r"(?m)^test result: (?:ok|FAILED)\. (\d+) passed;|^=+ (?:.*?, )?(\d+) passed(?:,.*)? in [0-9.]+s",
        )
        .unwrap()
    });
    let mut total = None;
    for capture in summary.captures_iter(output) {
        let count: u64 = capture
            .get(1)
            .or_else(|| capture.get(2))?
            .as_str()
            .parse()
            .ok()?;
        *total.get_or_insert(0) += count;
    }
    total
}

/// Wall time between two event stamps: `0.4s`, `12s`, `1m04s`, `2h05m`.
pub(super) fn elapsed(started: &str, finished: &str) -> Option<String> {
    let start = chrono::DateTime::parse_from_rfc3339(started).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(finished).ok()?;
    let millis = end.signed_duration_since(start).num_milliseconds().max(0);
    Some(if millis < 10_000 {
        format!("{:.1}s", millis as f64 / 1000.)
    } else if millis < 60_000 {
        format!("{}s", millis / 1000)
    } else if millis < 3_600_000 {
        format!("{}m{:02}s", millis / 60_000, millis / 1000 % 60)
    } else {
        format!("{}h{:02}m", millis / 3_600_000, millis / 60_000 % 60)
    })
}

pub(super) fn truncated(result: &Value) -> bool {
    let data = data(result);
    data["truncated"] == true || data["process"]["truncated"] == true
}

/// Source limits are metadata, not ordinary output rows. Compact presentation
/// must retain them even when all but one result row is folded by the UI.
pub(super) fn limitation(result: &Value) -> Option<&'static str> {
    if truncated(result) {
        return Some("工具结果已截断");
    }
    let data = data(result);
    if data["limit_reached"] == true
        || data["truncated_files"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
    {
        Some("搜索结果受限")
    } else {
        None
    }
}
pub(super) fn title(tool: &str, arguments: &Value) -> String {
    let arguments = crate::tools::approval::redact(arguments);
    let name = verb(tool);
    let detail = ["command", "path", "query", "url", "name"]
        .iter()
        .find_map(|k| arguments[*k].as_str())
        .unwrap_or("");
    let detail = if tool == "apply_patch" {
        arguments["edits"]
            .as_array()
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|e| e["path"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    } else {
        detail.lines().next().unwrap_or("").into()
    };
    super::state::clean(&format!("{name}  {detail}"))
        .chars()
        .take(500)
        .collect()
}
/// Result text, falling back to the structured data when there is no output.
pub(super) fn result(result: &Value) -> String {
    text(result, true)
}

/// Only output a person would read: streams, errors, file content, matches.
/// The compact tail under a tool line uses this; JSON stays in details.
pub(super) fn readable(result: &Value) -> String {
    text(result, false)
}

fn text(result: &Value, metadata: bool) -> String {
    if let Some(error) = result["error"].as_str() {
        return error.into();
    }
    let data = data(result);
    if let Some(content) = data["content"]
        .as_str()
        .filter(|_| data["path"].is_string())
    {
        let mut body = format!("{}\n", data["path"].as_str().unwrap_or(""));
        for (line, text) in content.lines().enumerate() {
            body.push_str(&format!("{:>5} │ {text}\n", line + 1));
        }
        if data["truncated"] == true {
            body.push_str("[文件内容已截断]\n");
        }
        return body;
    }
    if let Some(matches) = data["matches"].as_array() {
        let mut body = matches
            .iter()
            .map(|item| {
                format!(
                    "{}:{}  {}",
                    item["path"].as_str().unwrap_or(""),
                    item["line"]
                        .as_u64()
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    item["text"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if data["limit_reached"] == true {
            body.push_str("\n[搜索达到结果上限]");
        }
        if data["truncated_files"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
        {
            body.push_str(&format!("\n[截断文件：{}]", data["truncated_files"]));
        }
        return body;
    }
    if let Some(changes) = data["changes"].as_array() {
        return changes
            .iter()
            .map(|change| {
                format!(
                    "{}\n  {} → {}",
                    change["path"].as_str().unwrap_or(""),
                    change["before"].as_str().unwrap_or(""),
                    change["after"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut parts = ["stdout", "stderr", "output", "content", "text"]
        .iter()
        .filter_map(|key| data[*key].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(output) = data["process"]["output"].as_str() {
        parts.push(output.into());
    }
    if metadata && parts.is_empty() && !data.is_null() {
        parts.push(serde_json::to_string_pretty(data).unwrap_or_default());
    }
    if truncated(result) {
        parts.push("[工具已截断输出；日志文件保留情况见结果元数据]".into());
    }
    parts.join("\n")
}
