<img src="assets/brand/loh-icon.png" alt="Logit- Harness 蓝色 L 图标" width="72" height="72">

# Logit- Harness

启动命令：`loh`。一个以 DeepSeek Flash 为模型的 Rust Coding Agent，面向 macOS 本地开发。

普通对话与工程目标分开处理。问候和纯知识问答不扫描项目；项目问题按需只读查看，
明确的开发要求才启动工程执行。内置 `/` 命令与 Markdown Skills 共用命令入口，
完成的输出保留在终端原生历史里。具体完成项和验收证据见 [开发进度](docs/progress.md)。

任务由独立后台进程执行。终端可以随时输入问题或纠正，断开后仍继续工作。需求、执行动作、
文件指纹和验收证据一起持久化；需求变化会撤销过期结论，并取消受影响的工作，再根据实际
文件状态继续。普通回答结束后等待输入；工程目标则由独立上下文核验当前要求后才能完成。

## 开始使用

开发环境需要 Rust 1.89 或以上。浏览器工具需要 Google Chrome；文件、终端和模型循环可单独使用。

```sh
cargo build --release --locked
./target/release/loh doctor
```

运行设置向导，填写 API 地址和密钥（输入会隐藏）：

```sh
./target/release/loh setup
./target/release/loh
```

也可在 `loh` 中输入 `/setup`。Enter 从地址切换到密钥，再按 Enter 保存，Esc 取消。
设置仅在本机校验和保存，不会发起收费模型请求。仍支持 `DEEPSEEK_API_KEY` 环境变量，
它优先于保存的密钥；密钥不要放入命令参数或聊天消息。

也可以直接给出任务，当前目录就是工作目录：

```sh
loh run "为这个项目实现 SQLite 存储，并验证原有功能"
loh run "实现一个 JSON-lines CLI" --detach
loh sessions
loh attach <会话ID>
loh pause <会话ID>
loh resume <会话ID>
loh stop <会话ID>
```

直接运行 `loh` 会打开简洁的对话输入界面；`/sessions` 查看最近会话。`/` 筛选命令，Tab 补全，↑↓ 找回输入；
Ctrl+O 打开可搜索、分页和逐项展开的完整历史。没有密钥或后台进程时也能查看保存的记录。
有团队时，活动区常驻成员栏，Ctrl-T 切换；当前任务、控制范围和等待原因保持可见。
成功步骤默认一行，失败直接显示退出码与错误尾部；Ctrl-O 查看完整记录。详见 [终端操作](docs/terminal.md)。
蓝色 L 仅在客户端启动时显示一次；进入对话后，输入栏和用量页脚贴住终端底部。
普通对话、只读 Plan 与 Engineering 使用各自的输入提示，`/new` 后仍保持底部输入。
工具默认显示简短结果和失败尾部，完整原文、来源与技术诊断保留在详情中。

| 操作 | 命令 |
|---|---|
| 新对话，旧任务继续后台 | `/new`（`/clear` 同义） |
| 切换会话、恢复执行 | `/sessions`、`/resume` |
| 只读规划、明确目标、侧问 | `/plan`、`/goal`、`/btw` |
| 状态、后台工作、权限 | `/status`、`/tasks`、`/permissions` |
| 团队总览、切换成员、定向消息 | `/team`、`/agent <成员>`、`/tell <成员> <内容>` |
| 上下文、压缩、用量与 KV 命中 | `/context`、`/compact`、`/usage` |
| 完整历史、工具能力、复制、Markdown 导出 | `/history`、`/tools`、`/copy`、`/export session.md` |
| 技能、MCP、初始化与审查 | `/skills`、`/mcp`、`/init`、`/review` |
| 新会话默认值、当前预算、API 设置 | `/config`、`/budget`、`/setup` |

`/help` 查看完整用法和执行时机。`/plan` 保持只读，直到你明确要求实施；Skill 内容不能自行
退出规划。工作区被另一工程会话占用时，界面显示拥有者并等待执行权；新对话仍可使用。

`run --json` 和 `attach --json` 输出结构化事件。会话 ID 可使用唯一前缀。
`Ctrl-C` 请求暂停；`/detach` 断开终端；关闭终端也只断开客户端。
进程意外退出后，`resume` 会先核对日志、文件和进程身份，不盲目重放中断的写入。

工程目标可以由一支团队协作：主控负责分工和最终验收，成员拥有独立任务、上下文和消息。
默认最多两个成员同时执行，所有窗口共享一个后台会话和预算。成员并行分析，源码修改经
统一入口检查任务归属、可写范围、需求版本和文件指纹；用户编辑不会被旧补丁直接覆盖。

```text
/team
/agent <成员>
/agent main
/agent <成员> --window
/tell <成员> 请与另一成员确认接口
/team limit 2
```

也可从 VS Code 或其他终端运行 `loh attach <会话ID> --agent <成员>`。
主窗口的 Ctrl-C、`/pause`、`/resume`、`/stop` 控制整队，成员窗口只控制该成员；
显式 `/team pause|resume|stop` 控制整队。自动分派不会弹出窗口。历史、复制和压缩默认
跟随当前成员视图，预算、权限、目标和模式的修改回到主窗口。详细行为见
[团队操作](docs/terminal.md#working-with-a-team)。

## 配置与扩展

默认状态目录为 `~/.loh`，可用 `--home` 或 `LOH_HOME` 指定。`config.toml` 只保存非敏感配置：

```toml
model = "deepseek-flash"
tokens = 5000000
seconds = 21600
workers = 2
search_limit = 50
```

`/setup` 将密钥单独保存到状态目录的 `credentials.json`，目录权限为 700、文件权限为 600。
这是受文件权限保护的明文文件，并非加密存储或 macOS 钥匙串。每个密钥绑定完整 API Base URL；
更换地址需要该地址对应的密钥。留空保留已有密钥。新会话读取新的默认配置；已有会话重启时
保留其模型、地址和执行参数，同一地址的密钥则由新后台重新读取。`loh doctor` 可检查是否已
配置，但不代表已经联网验证。

所有角色共享任务 token 和时间限额。达到限额后保存未完成状态；`/budget <tokens> [seconds]`
设置绝对限额，再 `/resume`。搜索与抓取计次单独持久化。只有配置了明确费率时才计算费用估计。

终端页脚显示 KV 命中率；`/usage` 和 `/budget` 查看命中/未命中 token。固定指令与变化状态
分离，状态追加到历史末尾，减少前缀变化。统计口径、缓存基线与验证边界见 [KV Cache](docs/cache.md)。

内置文件搜索、读取、结构化补丁、终端、受管理的服务、联网搜索/抓取、Chrome DOM/无障碍树/
截图与输入操作。MCP 使用官方 Rust SDK，支持 stdio 和 Streamable HTTP。项目和用户的
`SKILL.md` 按需加载。说明见 [扩展配置](docs/extensions.md) 和 [终端操作](docs/terminal.md)。

本地开发操作自动执行。外部修改通过具体动作授权；浏览器授权还绑定实际来源。
Shell 的应用层规则不构成任意代码隔离。受管理的验证和开发服务使用 macOS 文件写入限制，
将构建产物放到独立输出目录。

## 验证与研究状态

```sh
cargo test --all-targets
loh eval status
```

工程测试、真实 API 冒烟和对照实验分别记录，不能相互代替。当前 369 项完整工程测试通过。
缓存优化前的冻结版本通过了 36 项发布模式测试和真实 JSON→SQLite 中途纠正验收；缓存优化
效果仍需要受控比较，实际请求的 KV 命中单独记录。完整对照实验受本次总预算限制，**尚未证明
动态纠偏比最强基线更可靠或更省人工**。开发集完成 20/180 次、保留集完成 15/360 次；原始
失败记录、冻结清单和账本均保留。详见 [工程验收](docs/acceptance.md) 和
[研究结果](docs/research-results.md)。运行与离线回放方式见 [实验执行](docs/eval-execution.md)。

本机终端已做实际 PTY 输入、中文插话、暂停、断开和恢复验证；Apple Terminal 原生窗口的
视觉检查尚未完成。当前针对 macOS，不宣称已经完成其他操作系统验收。

多窗口团队的最终安装版通过编辑、布局、交互及三个 PTY 客户端并发控制检查。原生 Terminal
窗口访问被 Computer Use 安全检查拒绝；真实 Flash 团队协作受共享账本可用额度限制，均保留
为未验收项。具体行为、审查修复、源码与二进制标识见 [团队验收记录](docs/team-acceptance.md)。
