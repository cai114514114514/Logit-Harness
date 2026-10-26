//! Display policy only. Raw events, identities and full history stay authoritative.
use super::Screen;
use crate::{commands::Timing, runtime::conversation::Mode};

pub(super) fn placeholder(screen: &Screen, width: usize) -> String {
    if screen.view.readonly {
        return "归档只读 · /history · /agent main".into();
    }
    if !screen.view.is_main() {
        let name = screen
            .member
            .as_ref()
            .map_or("当前成员", |member| member.name.as_str());
        return format!("给 {} 的补充，或输入 /", super::state::clean(name));
    }
    let session = screen.session.as_ref();
    let completed = session.is_some_and(|s| s.status == crate::domain::Status::Completed);
    match session.map(|session| session.mode) {
        None if !screen.entered => "输入消息、问题，或需要实现的目标…",
        None => "输入新消息，或输入 /",
        Some(Mode::Conversation) => "继续提问，或输入 /",
        Some(Mode::Plan) => "补充只读规划，或输入 /",
        Some(Mode::Engineering) if completed => "有新目标或追问，直接输入",
        // `width` is the editor's own width: 35 at 40 columns, 55 at 60.
        Some(Mode::Engineering) if width < 36 => "补充或纠正…",
        Some(Mode::Engineering) if width < 56 => "补充或纠正，或输入 /",
        Some(Mode::Engineering) => "补充要求、纠正方向，或输入 /",
    }
    .into()
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
            | "user input classified and applied"
            // The contract card and the acceptance receipt say these in full.
            | "initial user requirements decomposed with exact provenance"
            | "acceptance candidate saved; checking settled controls and evidence"
            | "completion withheld; verification is incomplete"
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
        "every active requirement passed independent evidence-backed verification" => {
            "每条有效条款都有独立验收证据 · /contract"
        }
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
