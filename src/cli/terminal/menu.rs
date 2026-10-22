//! One command catalog drives discovery, completion and help text.
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: bool,
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "/setup",
        description: "设置 API 地址和密钥",
        arguments: false,
    },
    Command {
        name: "/status",
        description: "查看任务与当前进度",
        arguments: false,
    },
    Command {
        name: "/diff",
        description: "查看工作目录的代码改动",
        arguments: false,
    },
    Command {
        name: "/budget",
        description: "Token、KV Cache 与运行时间",
        arguments: false,
    },
    Command {
        name: "/pause",
        description: "暂停执行，保存当前进度",
        arguments: false,
    },
    Command {
        name: "/resume",
        description: "继续当前任务",
        arguments: false,
    },
    Command {
        name: "/sessions",
        description: "选择已有会话",
        arguments: false,
    },
    Command {
        name: "/answer",
        description: "答复待确认问题 · <ID> <答复>",
        arguments: true,
    },
    Command {
        name: "/approve",
        description: "授权具体操作 · <ID>",
        arguments: true,
    },
    Command {
        name: "/stop",
        description: "停止任务和所属进程",
        arguments: false,
    },
    Command {
        name: "/detach",
        description: "离开终端，后台继续执行",
        arguments: false,
    },
    Command {
        name: "/help",
        description: "查看快捷键和命令",
        arguments: false,
    },
];

pub fn matches(draft: &str) -> Vec<&'static Command> {
    if !draft.starts_with('/') || draft.chars().any(char::is_whitespace) {
        return vec![];
    }
    COMMANDS
        .iter()
        .filter(|command| command.name.starts_with(draft))
        .collect()
}

pub fn help() -> String {
    let commands = COMMANDS
        .iter()
        .map(|command| format!("{}  {}", command.name, command.description))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{commands}\n\nEnter 发送 · Alt+Enter 换行 · ↑↓ 输入历史\n/ 打开命令菜单 · Tab 补全 · Esc 关闭菜单\nCtrl+O 展开工具输出 · PgUp/PgDn 翻阅\nCtrl+C 暂停 · Ctrl+D（空输入时）断开，任务继续\n/budget <总 token 上限> [总秒数上限] 调整任务额度"
    )
}
