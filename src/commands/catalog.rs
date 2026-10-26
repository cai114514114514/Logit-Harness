#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Team,
    Agent,
    Tell,
    Setup,
    Status,
    Diff,
    Budget,
    Pause,
    Resume,
    Sessions,
    Answer,
    Approve,
    Stop,
    Detach,
    Help,
    New,
    Rename,
    Export,
    Context,
    Compact,
    Usage,
    Plan,
    Goal,
    Tasks,
    Btw,
    History,
    Contract,
    Tools,
    Copy,
    Skills,
    Mcp,
    Config,
    Permissions,
    Doctor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    Local,
    Runtime,
    Settlement,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Arguments {
    None,
    Optional,
    Required,
    Budget,
    Answer,
    One,
    Skills,
    Mcp,
    Config,
}

pub struct Entry {
    pub kind: Kind,
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub example: &'static str,
    pub source: &'static str,
    pub timing: Timing,
    pub arguments: bool,
    pub(crate) rule: Arguments,
}

macro_rules! entry {
    ($kind:ident, $name:literal, $description:literal, $usage:literal, $example:literal, $rule:ident, $timing:ident) => {
        Entry {
            kind: Kind::$kind,
            name: $name,
            description: $description,
            usage: $usage,
            example: $example,
            source: "builtin",
            timing: Timing::$timing,
            arguments: !matches!(Arguments::$rule, Arguments::None),
            rule: Arguments::$rule,
        }
    };
}

pub const DIRECTORY: &[Entry] = &[
    entry!(
        Team,
        "/team",
        "成员、任务与全局团队控制",
        "/team [pause|resume|stop|limit N]",
        "/team",
        Optional,
        Runtime
    ),
    entry!(
        Agent,
        "/agent",
        "切换成员视图或打开成员窗口",
        "/agent NAME [--window]",
        "/agent main",
        Required,
        Local
    ),
    entry!(
        Tell,
        "/tell",
        "向指定成员发送持久消息",
        "/tell NAME TEXT",
        "/tell Ada 检查接口",
        Answer,
        Runtime
    ),
    entry!(
        Setup,
        "/setup",
        "设置 API 地址和密钥",
        "/setup",
        "/setup",
        None,
        Local
    ),
    entry!(
        Status,
        "/status",
        "查看任务与当前进度",
        "/status",
        "/status",
        None,
        Local
    ),
    entry!(
        Diff,
        "/diff",
        "查看工作目录代码改动",
        "/diff",
        "/diff",
        None,
        Local
    ),
    entry!(
        Budget,
        "/budget",
        "当前任务 token 和时间额度",
        "/budget [tokens|unlimited] [seconds|unlimited]",
        "/budget",
        Budget,
        Runtime
    ),
    entry!(
        Pause,
        "/pause",
        "暂停执行并保存进度",
        "/pause",
        "/pause",
        None,
        Settlement
    ),
    entry!(
        Resume,
        "/resume",
        "恢复当前任务执行",
        "/resume",
        "/resume",
        None,
        Runtime
    ),
    entry!(
        Sessions,
        "/sessions",
        "选择已有会话",
        "/sessions",
        "/sessions",
        None,
        Local
    ),
    entry!(
        Answer,
        "/answer",
        "答复待确认问题",
        "/answer <ID> <答复>",
        "/answer id 是",
        Answer,
        Runtime
    ),
    entry!(
        Approve,
        "/approve",
        "授权具体操作",
        "/approve <ID>",
        "/approve id",
        One,
        Runtime
    ),
    entry!(
        Stop,
        "/stop",
        "停止任务和所属进程",
        "/stop",
        "/stop",
        None,
        Settlement
    ),
    entry!(
        Detach,
        "/detach",
        "离开终端，后台继续",
        "/detach",
        "/detach",
        None,
        Local
    ),
    entry!(
        Help,
        "/help",
        "查看命令、来源与执行时机",
        "/help",
        "/help",
        None,
        Local
    ),
    entry!(
        New,
        "/new",
        "新聊天，旧任务继续后台",
        "/new",
        "/new",
        None,
        Local
    ),
    entry!(
        New,
        "/clear",
        "/new 的别名",
        "/clear",
        "/clear",
        None,
        Local
    ),
    entry!(
        Rename,
        "/rename",
        "重命名当前会话",
        "/rename <标题>",
        "/rename 新标题",
        Required,
        Runtime
    ),
    entry!(
        Export,
        "/export",
        "导出 Markdown（.json 为完整事件），拒绝覆盖",
        "/export <文件路径>",
        "/export session.md",
        Required,
        Local
    ),
    entry!(
        Context,
        "/context",
        "上下文字节估算与有效需求",
        "/context",
        "/context",
        None,
        Local
    ),
    entry!(
        Compact,
        "/compact",
        "结算后压缩上下文",
        "/compact",
        "/compact",
        None,
        Settlement
    ),
    entry!(
        Usage,
        "/usage",
        "已报告 token 用量与 KV 命中",
        "/usage",
        "/usage",
        None,
        Local
    ),
    entry!(
        Plan,
        "/plan",
        "进入只读规划，等待明确实施",
        "/plan [内容]",
        "/plan",
        Optional,
        Settlement
    ),
    entry!(
        Goal,
        "/goal",
        "查看或提交目标及需求变更",
        "/goal [目标或变更]",
        "/goal",
        Optional,
        Runtime
    ),
    entry!(
        Tasks,
        "/tasks",
        "查看工作者、命令、服务与待办",
        "/tasks [ID]",
        "/tasks",
        Optional,
        Local
    ),
    entry!(
        Btw,
        "/btw",
        "独立只读侧问，保留当前目标",
        "/btw <问题>",
        "/btw 解释这个变量",
        Required,
        Runtime
    ),
    entry!(
        History,
        "/history",
        "浏览完整历史并搜索",
        "/history [tools] [搜索词]",
        "/history",
        Optional,
        Local
    ),
    entry!(
        Contract,
        "/contract",
        "查看契约：条款原话、修订链、证据与验收",
        "/contract",
        "/contract",
        None,
        Local
    ),
    entry!(
        Tools,
        "/tools",
        "查看工具能力、可用条件与权限边界",
        "/tools [名称或来源]",
        "/tools",
        Optional,
        Local
    ),
    entry!(
        Copy,
        "/copy",
        "选择并复制回答或工具内容",
        "/copy [ID]",
        "/copy",
        Optional,
        Local
    ),
    entry!(
        Skills,
        "/skills",
        "浏览技能或按名称/来源 ID 调用",
        "/skills [run <名称或ID> [参数]]",
        "/skills",
        Skills,
        Local
    ),
    entry!(
        Mcp,
        "/mcp",
        "查看或重连、启停已配置服务",
        "/mcp [inspect|reconnect|enable|disable <服务>]",
        "/mcp",
        Mcp,
        Settlement
    ),
    entry!(
        Config,
        "/config",
        "非敏感默认值，仅 /new 新会话生效",
        "/config [key [value]]",
        "/config",
        Config,
        Local
    ),
    entry!(
        Permissions,
        "/permissions",
        "策略与待授权的具体操作",
        "/permissions",
        "/permissions",
        None,
        Local
    ),
    entry!(
        Doctor,
        "/doctor",
        "本地环境与凭据来源检查",
        "/doctor",
        "/doctor",
        None,
        Local
    ),
];

pub fn matches(draft: &str) -> Vec<&'static Entry> {
    if !draft.starts_with('/') {
        return vec![];
    }
    let name = draft.split_whitespace().next().unwrap_or(draft);
    DIRECTORY
        .iter()
        .filter(|entry| {
            if draft.chars().any(char::is_whitespace) {
                entry.name == name
            } else {
                entry.name.starts_with(name)
            }
        })
        .collect()
}

pub fn help() -> String {
    DIRECTORY
        .iter()
        .map(|entry| {
            format!(
                "{}  {} [{} · {:?}]",
                entry.usage, entry.description, entry.source, entry.timing
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n/init [bundled Skill · 缺少指导时初始化，已有指导或 Plan 时仅建议]\n/review [bundled Skill · 只读审查]\n/<技能名> [字面参数]；同名技能用 /skills run <ID> 选择来源"
}
