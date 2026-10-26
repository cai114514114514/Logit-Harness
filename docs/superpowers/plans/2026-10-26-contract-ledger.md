# loh 契约账本终端 Implementation Plan

> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把设计稿《loh 契约账本》实现为真实的 loh 终端：条款、证据状态、修订和验收成为界面主体。

**Architecture:** 运行时只增加数据：两个新事件（`Contract`、`Acceptance`）、`SessionView.contract`、
`Activity` 的条款范围、`Session.acceptance`。终端沿用一个 `Screen`、一个事件投影和正常终端缓冲区，
先换颜色 token，再改 scrollback 呈现、活动区，最后加 `/contract` 详情。不新增平行渲染器。

**Tech Stack:** Rust、Ratatui、Crossterm、现有 Tokio IPC；Python pexpect/pyte PTY 检查。

**Spec:** `docs/superpowers/specs/2026-10-26-contract-ledger-design.md`（用户 2026-10-26 确认四个待定项）。
**画面:** `docs/superpowers/specs/2026-10-26-contract-ledger.html`。

## Global Constraints

- 在 `~/residual` 原工作树修改；不创建 worktree，不使用 sed。所有编译、测试都在本机 Mac 上进行。
- 可读、按职责拆分、必要处说明原因；不使用 V2、next 或冗长重复命名。
- 事件持久化契约只加变体和带 serde default 的字段，不改已有字段名、标签和形状。
- 运行时判定（完成屏障、证据当前性、验收、调度）不改；界面只呈现运行时已有的判断。
  `Evidence::current` / `current_at` 的结果必须保持不变。
- 普通对话不派工，开菜单、切换视图、`/contract` 都不调用模型。
- 原生历史只追加一次；卡片和发言一样走 `drain_settled` / `recent`，重连、缩放不重复写。
- 旧会话（没有新事件）能完整回放，界面退回现有一行写法。
- 不修改用户的 `docs/bug-review.md`，不动冻结研究和 50 元实验账本；离线 UI 验证不发起付费调用。
- 工程验收、交互验收和真实模型表现分别报告。

## Review Focus

1. 印章判定：失败后正在修、验收驳回后有新通过证据、epoch 推进、步骤失效、只有读取证据（Task 1）。
2. 修订卡必须来自同一次修订的真实结果：失效步骤、取消作业、过期证据（Task 1）。
3. 条款标签只在作用范围比全部有效条款窄时出现；主控未聚焦步骤时不标（Task 1、3）。
4. 卡片在实况、重连、`/history`、视图切换中各只出现一次（Task 3）。
5. OSC 11 查询不吞用户按键，不在非 TTY 或超时时阻塞启动（Task 2）。
6. 窄屏收起顺序：当前身份、控制范围、失败和输入光标始终保留（Task 4）。

---

## File map and task order

| 文件 | 责任 | 任务 |
|---|---|---|
| `src/domain/evidence.rs` | `current_version`，`current` 在其上比对文件 | 1 |
| 新 `src/domain/contract.rs` | 事件负载：条款记录、修订变更、验收检查 | 1 |
| `src/domain/event.rs` | 新变体 `Contract`、`Acceptance` | 1 |
| 新 `src/runtime/contract.rs` | `ContractView` 推导、`Runner::emit_contract`、修订前快照 | 1 |
| `meta.rs`、`steering.rs`、`questions.rs`、`team/corrections.rs`、`verify.rs` | 在已有位置发出新事件 | 1 |
| `src/runtime/view.rs`、`state.rs` | `SessionView.contract`、`Activity` 范围、`Session.acceptance` | 1 |
| 新 `src/cli/terminal/theme.rs`，`text.rs` | 颜色 token、NO_COLOR、OSC 11 背景查询 | 2 |
| `entry.rs`、`summary.rs`、`tool_text.rs`、`transcript.rs` | 发言竖条、工具行、成员前缀 | 3 |
| 新 `src/cli/terminal/ledger.rs` | 契约卡、修订卡、验收回执的呈现 | 3 |
| 新 `src/cli/terminal/contract_bar.rs`，`render.rs`、`status.rs`、`activity.rs`、`composer.rs`、`present.rs` | 契约栏、状态行、输入横线、页脚、启动说明、窄屏 | 4 |
| 新 `src/cli/terminal/contract.rs`，`commands/catalog.rs`、`client.rs`、`state.rs` | `/contract` 详情 | 5 |
| `tests/contract.rs`、`tests/terminal*.rs`、`tests/pty/*.py` | 行为契约和 PTY 网格 | 各任务、6 |
| `docs/terminal.md`、`docs/progress.md`、新验收记录 | 使用方式和证据 | 6 |

顺序执行；每个任务结束时 fmt、clippy、全量测试都通过。

### Task 1: 运行时契约数据

**Interfaces:**
- `Evidence::current_version(&self, goal: &Goal, steps: &BTreeMap<String, u64>) -> bool`：通过、
  覆盖至少一条条款、条款版本和 epoch 匹配、步骤版本匹配。不看文件。`current_at` 改为
  `current_version && files_match`，`current` 保持只看条款、epoch、文件（步骤不参与），结果不变。
- `domain::contract`：
  - `ClauseRecord { id, text, revision, active, source }`
  - `ContractChanges { updated: Vec<ClauseUpdate{id, before, after}>, added: Vec<String>, retired: Vec<String>,
    cancelled: Vec<CancelledAction{id, title, requirements, settled}>, stale: Vec<StaleEvidence{id, description, reason}>,
    kept: Vec<KeptStep{id, description}> }`，全部 serde default。
  - `AcceptanceCheck { requirement, passed, evidence: Vec<CitedEvidence{id, kind, description}>, reason }`
- `EventData::Contract { revision, epoch, source, cause, clauses: Vec<ClauseRecord>, changes: ContractChanges }`
  和 `EventData::Acceptance { revision, passed, checks: Vec<AcceptanceCheck>, summary }`。
- `runtime::contract::Before` 在修订前捕获：条款快照、步骤版本、当前证据 id 集合、运行中的作业（id、标题、条款）。
  `Runner::emit_contract(&mut self, cause: &str, before: Option<Before>)` 对比修订后状态生成 changes 并发出。
  `extract` 传 `None`（契约卡）。
- `SessionView.contract: Option<ContractView>`：`revision`、`clauses: Vec<ClauseView{id, text, revision, stamp, working, evidence}>`、
  `acceptance: Option<AcceptanceSummary>`。`Stamp` 枚举：`Open`、`Evidenced`、`Accepted`、`Stale`、`Failed`。
- `Activity` 加 `requirements: BTreeMap<String, u64>` 和 `focused: bool`（serde default）。
- `Session.acceptance: Option<domain::contract::Acceptance>`（serde default），`verify_report` 写入。

- [ ] 写失败测试 `tests/contract.rs`：五种印章、失败后正在修、驳回后新证据、epoch 推进使证据过期、步骤失效、
      只有读取证据为 `○`；`current`/`current_at` 对既有用例结果不变。
- [ ] 实现 domain 类型和事件变体；`eval/report.rs` 等穷举匹配补上新变体。
- [ ] 实现 `ContractView` 推导和 `Activity` 范围。
- [ ] 四处修订点和抽取点调用 `emit_contract`；`verify.rs` 发出 `Acceptance` 并写 `Session.acceptance`。
- [ ] 测试：refine 后 `Contract` 事件列出更新、新增、取消的作业、过期证据和保留步骤，与 `graph.invalidate`、
      被取消的 job 一致；replace 列出退役条款；验收驳回和通过各发一次 `Acceptance`。
- [ ] 旧会话 JSON（无新字段）反序列化通过。fmt、clippy、全量测试。

### Task 2: 颜色 token 与浅色终端

- [ ] 新 `theme.rs`：`accent()`、`fresh()`、`stale()`、`fail()`、`muted()`、`member(index)`，返回 `Color`；
      `NO_COLOR` 时全部为 `Color::Reset`。`text::ACCENT` / `MUTED` 常量的所有使用点改为函数。
- [ ] `theme::detect(timeout)`：启动、进入 raw mode 之后、开始读输入之前查询 `OSC 11`；解析 `rgb:R/G/B`
      （1–4 位十六进制），相对亮度大于 0.5 判为浅色。查询期间读到的其他字节放回输入队列。
- [ ] 单元测试：解析各种回复格式、亮度阈值、NO_COLOR、超时无回复保持默认。
- [ ] fmt、clippy、全量测试。

### Task 3: scrollback 呈现

- [ ] `entry.rs`：发言竖条取代标题行（▌ 你、│ 主控、┃ 成员、║ 验收），每个视觉行加竖条；成员在主控视图带名字前缀。
- [ ] `tool_text.rs`：中文动词；右侧结果（行数、结果数、`+增 −删`、退出码、可解析的测试汇总）、耗时、条款标签。
      `transcript.rs` 从 `ActionScoped` 记录条款范围，从最新 `Contract` 事件记录全部有效条款以判断“更窄”。
- [ ] 新 `ledger.rs`：`Contract`、`Acceptance` 事件生成卡片条目（新 `Kind::Ledger`），走同一套结算和提交；
      验收者抬头统计 verifier 的读取和检查次数及用时。旧会话的 `Revised` 一行写法保留，有 `Contract` 事件时不再重复。
- [ ] 更新 `tests/terminal*.rs`、`team_tests.rs`、`review_tests.rs` 中受影响的断言；新增卡片和工具行的网格测试
      （80、60、40 列）。fmt、clippy、全量测试。

### Task 4: 活动区

- [ ] 新 `contract_bar.rs`：契约栏三种宽度形态、成员视图形态；`render.rs` 把它放在团队栏之后。
- [ ] `activity.rs` / `status.rs`：状态行格式（动词、对象、服务条款、已用、距输出）；团队主控每个忙碌成员一行；
      空闲不再出状态行。
- [ ] `composer.rs`：单条上横线和标签、`▸`、去掉下横线；页脚左右对齐和窄屏收起。`present.rs`：启动说明和横线标签。
- [ ] 更新 `tests/terminal_layout.rs`、`terminal_chrome.rs`、`terminal_activity.rs`；四种尺寸网格测试。
      fmt、clippy、全量测试。

### Task 5: /contract 详情

- [ ] `commands/catalog.rs` 增加 `/contract`（本地操作）；`client.rs` 打开临时全屏；Esc 返回不重播历史。
- [ ] 新 `contract.rs`：条款列表、来源、修订链、证据（序号 `eN`、当前与否和原因）、验收结论、进行中的动作；
      已退役条款在最后；↑↓、Enter 展开证据原文、`/` 搜索、`c` 复制来源原话。
- [ ] `/status` 文案改为“契约 v{n} · {m} 条条款”。
- [ ] 测试：无契约、修订后、退役条款、展开与复制。fmt、clippy、全量测试。

### Task 6: 回归、PTY 网格与文档

- [ ] `tests/pty/layout.py` 等更新预期；新增契约场景的离线 fixture（不调用模型），在 110×34、80×24、60×18、
      40×12 抓取网格，深色、浅色（模拟 OSC 11 回复）、NO_COLOR 各一遍，与设计稿逐帧对照并记录差异。
- [ ] 全量测试、fmt、严格 Clippy、release 构建。
- [ ] 更新 `docs/terminal.md`、`docs/progress.md`，新增 `docs/contract-ledger-acceptance.md` 记录证据。
- [ ] 安装新二进制前先征得用户同意。
