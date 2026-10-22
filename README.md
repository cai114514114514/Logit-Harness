<img src="assets/brand/loh-icon.png" alt="Logit- Harness 蓝色 L 图标" width="72" height="72">

# Logit- Harness

启动命令：`loh`。一个以 DeepSeek Flash 为模型的 Rust Coding Agent，面向 macOS 本地开发。

任务由独立后台进程执行。终端可以随时输入问题或纠正，断开后仍继续工作。需求、执行动作、
文件指纹和验收证据一起持久化；需求变化会撤销过期结论，并取消受影响的工作，再根据实际
文件状态继续。模型回复结束后，由独立上下文核验当前要求，才能进入完成状态。

## 开始使用

开发环境需要 Rust 1.89 或以上。浏览器工具需要 Google Chrome；文件、终端和模型循环可单独使用。

```sh
cargo build --release --locked
./target/release/loh doctor
```

启动环境中需要 `DEEPSEEK_API_KEY`。例如在 zsh 中隐式输入密钥，避免写进命令历史：

```sh
read -rs 'DEEPSEEK_API_KEY?DeepSeek API key: '
export DEEPSEEK_API_KEY
./target/release/loh
```

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

直接运行 `loh` 会打开任务输入与最近会话界面。`/` 筛选命令，Tab 补全，↑↓ 找回输入；
Ctrl+O 展开工具结果。没有密钥或后台进程时也能查看保存的记录。详见 [终端操作](docs/terminal.md)。

`run --json` 和 `attach --json` 输出结构化事件。会话 ID 可使用唯一前缀。
`Ctrl-C` 请求暂停；`/detach` 断开终端；关闭终端也只断开客户端。
进程意外退出后，`resume` 会先核对日志、文件和进程身份，不盲目重放中断的写入。

## 配置与扩展

默认状态目录为 `~/.loh`，可用 `--home` 或 `LOH_HOME` 指定。`config.toml` 只保存非敏感配置：

```toml
model = "deepseek-flash"
tokens = 5000000
seconds = 21600
workers = 2
search_limit = 50
```

所有角色共享任务 token 和时间限额。达到限额后保存未完成状态；`/budget <tokens> [seconds]`
设置绝对限额，再 `/resume`。搜索与抓取计次单独持久化。只有配置了明确费率时才计算费用估计。

终端顶栏显示 `KV hit`；`/budget` 查看输入缓存命中率和命中/未命中 token。固定指令与变化状态
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

工程测试、真实 API 冒烟和对照实验分别记录，不能相互代替。当前 143 项完整工程测试通过。
缓存优化前的冻结版本通过了 36 项发布模式测试和真实 JSON→SQLite 中途纠正验收；缓存优化
后的真实 API 命中率尚待测量。完整对照实验受本次总预算限制，**尚未证明
动态纠偏比最强基线更可靠或更省人工**。开发集完成 20/180 次、保留集完成 15/360 次；原始
失败记录、冻结清单和账本均保留。详见 [工程验收](docs/acceptance.md) 和
[研究结果](docs/research-results.md)。运行与离线回放方式见 [实验执行](docs/eval-execution.md)。

本机终端已做实际 PTY 输入、中文插话、暂停、断开和恢复验证；Apple Terminal 原生窗口的
视觉检查尚未完成。当前针对 macOS，不宣称已经完成其他操作系统验收。
