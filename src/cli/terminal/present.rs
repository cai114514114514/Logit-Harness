//! Display policy only. Raw events, identities and full history stay authoritative.
use super::Screen;
use crate::{commands::Timing, runtime::conversation::Mode};

pub(super) fn placeholder(screen: &Screen) -> &'static str {
    if screen.view.readonly {
        return "归档只读 · /history · /agent main";
    }
    if !screen.view.is_main() {
        return "发给当前成员 · /pause 仅暂停此成员 · /agent main";
    }
    match screen.session.as_ref().map(|session| session.mode) {
        None if !screen.entered => "输入消息、问题，或需要实现的目标…",
        None => "输入新消息，或输入 /",
        Some(Mode::Conversation) => "继续提问，或输入 /",
        Some(Mode::Plan) => "补充只读规划，或输入 /",
        Some(Mode::Engineering) => "补充要求、纠正方向，或输入 /",
    }
}

pub(super) fn mode(screen: &Screen) -> &'static str {
    match screen.session.as_ref().map(|session| session.mode) {
        None | Some(Mode::Conversation) => "对话",
        Some(Mode::Plan) => "Plan · 只读",
        Some(Mode::Engineering) => "Engineering",
    }
}

pub(super) fn timing(timing: Timing) -> &'static str {
    match timing {
        Timing::Local => "本地操作",
        Timing::Runtime => "立即提交",
        Timing::Settlement => "结算后执行",
    }
}

/// Hide only known routine notices. Unknown diagnostics and all failures remain
/// visible, so a new runtime error cannot disappear behind a broad keyword rule.
pub(super) fn routine(message: &str) -> bool {
    // Addressed success already has a structured, target-labelled receipt. Keep
    // its raw diagnostic searchable without printing another pair of UUIDs.
    let directed = message
        .strip_prefix("directed request ")
        .and_then(|message| {
            message
                .strip_suffix(": Applied")
                .or_else(|| message.strip_suffix(": Pending"))
        })
        .or_else(|| {
            message
                .strip_prefix("directed input user:")
                .and_then(|message| message.strip_suffix(": applied"))
        });
    if directed.is_some_and(|key| serde_json::from_str::<[String; 2]>(key).is_ok()) {
        return true;
    }
    matches!(
        message,
        "verified goal archived; ready for a new conversation turn"
            | "compaction queued at a fully paired tool boundary; durable requirements and corrections remain authoritative"
    ) || message.ends_with(" context compacted; current requirements and pending work retained")
}

pub(super) fn notice(message: &str) -> String {
    let Some((status, reason)) = message.split_once(" · ") else {
        return message.into();
    };
    let explanation = match reason {
        "settling active operations before changing execution mode" => "正在结算活动，随后切换模式",
        "execution boundary settled" => "活动已结算",
        "pause requested; cancelling active operations" => "正在取消活动",
        "all active operations have settled" => "活动已结算",
        "independent work is settled; dependent steps await user input" => {
            "当前工作已结算，等待答复 · /status 查看待答问题"
        }
        "waiting for scoped authorization" => "等待授权 · /status 查看详情",
        _ => return message.into(),
    };
    format!("{status} · {explanation}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn structured_success_diagnostic_is_quiet_but_errors_remain_visible() {
        assert!(super::routine(
            "directed request [\"client\",\"request\"]: Applied"
        ));
        assert!(super::routine(
            "directed input user:[\"client\",\"request\"]: applied"
        ));
        assert!(!super::routine(
            "directed request [\"client\",\"request\"]: Failed (stale assignment)"
        ));
        assert!(!super::routine("directed request unknown: Applied"));
    }
}
