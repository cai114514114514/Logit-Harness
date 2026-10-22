//! Display policy only. Raw events, identities and full history stay authoritative.
use super::Screen;
use crate::{commands::Timing, runtime::conversation::Mode};

pub(super) fn placeholder(screen: &Screen) -> &'static str {
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
