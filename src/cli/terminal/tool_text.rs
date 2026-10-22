//! Tool output is data, never Markdown. Preserve command bytes and typed results.
use serde_json::Value;

fn data(result: &Value) -> &Value {
    result
        .get("result")
        .and_then(|value| value.get("data"))
        .or_else(|| result.get("data"))
        .unwrap_or(result)
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
    let name = match tool {
        "read_file" => "Read",
        "search_files" | "search" => "Search",
        "apply_patch" => "Edit",
        "shell" => "Run",
        "check" => "Check",
        _ => tool,
    };
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
pub(super) fn result(result: &Value) -> String {
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
    if parts.is_empty() && !data.is_null() {
        parts.push(serde_json::to_string_pretty(data).unwrap_or_default());
    }
    if truncated(result) {
        parts.push("[工具已截断输出；日志文件保留情况见结果元数据]".into());
    }
    parts.join("\n")
}
