//! Durable Markdown serialization, independent of terminal layout and truncation.
use crate::{
    domain::{EventData, SessionEvent},
    runtime::state::Session,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Origin<'a> {
    actor: &'a str,
    turn: &'a str,
    response: &'a str,
}

pub(super) fn markdown(session: &Session, sequence: u64, events: &[SessionEvent]) -> String {
    let title = if session.title.is_empty() {
        "会话"
    } else {
        &session.title
    };
    let mut output = format!(
        "# {}\n\n- 会话：`{}`\n- 工作区：`{}`\n- 状态：`{:?}`\n- 事件范围：至 {}\n\n",
        title.replace(['\n', '\r'], " "),
        session.id,
        session.workspace.display(),
        session.status,
        sequence
    );
    let mut origins = BTreeMap::<&str, Origin<'_>>::new();
    let mut starts = BTreeMap::<&str, &SessionEvent>::new();
    let mut streams = BTreeMap::<&str, (&SessionEvent, String)>::new();
    for event in events {
        match &event.data {
            EventData::Identity {
                actor,
                response,
                turn,
                action,
                ..
            } => {
                if action.is_none()
                    && let Some((start, text)) = streams.remove(actor.as_str())
                {
                    partial(
                        &mut output,
                        actor,
                        start,
                        &text,
                        origins.get(actor.as_str()).copied(),
                    );
                }
                origins.insert(
                    action.as_deref().unwrap_or(actor),
                    Origin {
                        actor,
                        turn,
                        response,
                    },
                );
            }
            EventData::Created {
                objective: text, ..
            }
            | EventData::User { message: text }
                if !text.is_empty() =>
            {
                heading(&mut output, "用户", "user", event, None);
                fence(&mut output, "text", text);
            }
            EventData::Message { actor, text } => {
                streams.remove(actor.as_str());
                heading(
                    &mut output,
                    "回答",
                    actor,
                    event,
                    origins.get(actor.as_str()).copied(),
                );
                // Preserve the authored Markdown response; tool bytes below are
                // fenced separately and cannot close their own containing fence.
                output.push_str(text);
                output.push_str("\n\n");
            }
            EventData::Delta { actor, text } => {
                streams
                    .entry(actor)
                    .or_insert_with(|| (event, String::new()))
                    .1
                    .push_str(text);
            }
            EventData::ActionStarted { id, .. } => {
                starts.insert(id, event);
            }
            EventData::ActionFinished {
                id,
                success,
                result,
            } => {
                let stream = streams.remove(id.as_str());
                let origin = origins.get(id.as_str()).copied();
                let started = starts.remove(id.as_str());
                tool_heading(
                    &mut output,
                    id,
                    event,
                    started,
                    origin,
                    if *success { "成功" } else { "未成功" },
                );
                // ProcessOutput is a bounded tail, whereas Delta events retain
                // every emitted chunk. Keep both records without substituting
                // the shorter final payload for the persisted stream.
                tool_stream(&mut output, stream.as_ref().map(|(_, text)| text.as_str()));
                raw_tool_text(&mut output, result);
                output.push_str("完整结构化工具结果（输出字段可能由执行器限长）：\n\n");
                fence(&mut output, "json", &json(result));
            }
            EventData::Status { status, reason } => {
                heading(
                    &mut output,
                    &format!("状态 · {status:?}"),
                    "系统",
                    event,
                    None,
                );
                fence(&mut output, "text", reason);
            }
            _ => (),
        }
    }
    // An unmatched start is itself durable history, even for a quiet process
    // that has emitted no Delta. Do not claim it finished or is still alive:
    // only its un-settled status at this export boundary is known.
    let mut pending = starts.into_iter().collect::<Vec<_>>();
    pending.sort_by_key(|(_, event)| event.sequence);
    for (id, started) in pending {
        tool_heading(
            &mut output,
            id,
            started,
            Some(started),
            origins.get(id).copied(),
            "未结算",
        );
        output.push_str(&format!("截至导出事件 {sequence}，未收到最终结果。\n\n"));
        let stream = streams.remove(id);
        tool_stream(&mut output, stream.as_ref().map(|(_, text)| text.as_str()));
    }
    // Keep orphan/model streams too; those have no start record to describe.
    for (actor, (start, text)) in streams {
        partial(
            &mut output,
            actor,
            start,
            &text,
            origins.get(actor).copied(),
        );
    }
    output
}

fn tool_heading(
    output: &mut String,
    id: &str,
    event: &SessionEvent,
    started: Option<&SessionEvent>,
    origin: Option<Origin<'_>>,
    status: &str,
) {
    let name = started
        .and_then(|start| {
            if let EventData::ActionStarted { tool, .. } = &start.data {
                Some(tool.as_str())
            } else {
                None
            }
        })
        .unwrap_or("工具");
    heading(
        output,
        &format!("工具 · {name} · {status}"),
        origin.map_or(id, |value| value.actor),
        event,
        origin,
    );
    output.push_str(&format!("- 操作：`{id}`\n"));
    if let Some(start) = started {
        output.push_str(&format!(
            "- 开始：{} · 事件 {}\n",
            start.timestamp, start.sequence
        ));
        if let EventData::ActionStarted { arguments, .. } = &start.data {
            output.push_str("\n请求参数：\n\n");
            fence(output, "json", &json(arguments));
        }
    }
}
fn tool_stream(output: &mut String, text: Option<&str>) {
    if let Some(text) = text {
        output.push_str("完整已持久化流输出：\n\n");
        fence(output, "text", text);
    } else {
        output.push_str("尚无已持久化流输出。\n\n");
    }
}

fn heading(
    output: &mut String,
    title: &str,
    actor: &str,
    event: &SessionEvent,
    origin: Option<Origin<'_>>,
) {
    output.push_str(&format!(
        "## {title}\n\n- 来源：`{actor}`\n- 时间：{}\n- 事件：{}\n",
        event.timestamp, event.sequence
    ));
    if let Some(origin) = origin {
        output.push_str(&format!(
            "- 回合：`{}`\n- 响应：`{}`\n",
            origin.turn, origin.response
        ));
    }
    output.push('\n');
}
fn partial(
    output: &mut String,
    actor: &str,
    event: &SessionEvent,
    text: &str,
    origin: Option<Origin<'_>>,
) {
    heading(
        output,
        "未结算流输出",
        origin.map_or(actor, |value| value.actor),
        event,
        origin,
    );
    output.push_str(&format!("- 流标识：`{actor}`\n\n"));
    fence(output, "text", text);
}
fn json(value: &serde_json::Value) -> String {
    // JSON Values are always serializable; no loss or display-size cap is applied.
    serde_json::to_string_pretty(value).expect("JSON value serialization")
}
fn raw_tool_text(output: &mut String, result: &serde_json::Value) {
    let data = result
        .get("result")
        .and_then(|value| value.get("data"))
        .or_else(|| result.get("data"))
        .unwrap_or(result);
    for key in ["stdout", "stderr", "output", "content", "text"] {
        if let Some(text) = data[key].as_str() {
            output.push_str(&format!("最终结果中的原始 {key}：\n\n"));
            fence(output, "text", text);
        }
    }
    if let Some(text) = data["process"]["output"].as_str() {
        output.push_str("最终结果中的进程输出（执行器可能只保留尾部）：\n\n");
        fence(output, "text", text);
    }
    if let Some(text) = result["error"].as_str() {
        output.push_str("最终结果中的原始错误：\n\n");
        fence(output, "text", text);
    }
}
fn fence(output: &mut String, language: &str, text: &str) {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let marker = "`".repeat((longest + 1).max(3));
    output.push_str(&format!("{marker}{language}\n"));
    output.push_str(text);
    if !text.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&format!("{marker}\n\n"));
}
