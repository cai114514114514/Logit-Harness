# Using the Logit- Harness terminal

The executable is `loh`. In a development checkout, build with `cargo build` and run
`./target/debug/loh run "任务描述"`. Export `DEEPSEEK_API_KEY` in the launching environment.
Configuration and session state default to `~/.loh`, configurable with `LOH_HOME` or `--home`.

When stdin and stdout are terminals, `run` and `attach` open the Ratatui interface. With `--json`,
they stream structured messages. Redirected non-JSON output remains readable text. `run --detach`
starts the owned background daemon and returns its session ID. `loh sessions` lists saved work;
`loh attach <id>` reconnects. If a daemon has exited, `loh resume <id>` reconciles it before work
continues. Only one runner owns a workspace at a time.

Type a correction or question at any time. Enter submits; Alt+Enter inserts a newline. Pasted
multiple lines remain a draft until Enter. Editing respects grapheme boundaries for Chinese,
combining marks and joined emoji. PageUp/PageDown scroll the transcript.

| Command | Behavior |
|---|---|
| `/status` | Read the current saved session state |
| `/diff` | Show the current Git worktree, including pre-existing user changes |
| `/budget` | Show consumed, reserved and uncertain token use, and elapsed time |
| `/budget <tokens> [seconds]` | Set absolute task limits; never changes the shared campaign money cap |
| `/pause` or Ctrl-C | Request cancellation and pause after active operations settle |
| `/resume` | Resume saved execution |
| `/answer <question-id> <text>` | Answer one pending question explicitly |
| `/approve <authorization-id>` | Approve the exact requested action |
| `/stop` | Stop the session and its managed processes |
| `/detach` or Ctrl-D | Disconnect this terminal; background execution continues |
| `/help` | Show command usage |

An authorization request displays the concrete tool, redacted arguments, current origin where
applicable, and a private JSON review-file path for the full preview. The ID binds the original
action; approving it queues that action for a fresh scope check. A changed origin requires a new
approval. Known credential fields and common credential forms are redacted in the preview.

Closing the terminal only disconnects the client. Controls become durable when the socket server
acknowledges them. If delivery is unconfirmed, the interface restores message text where possible;
inspect the session before resubmitting to avoid duplicates. The display does not rewind its state
when replaying older events or receiving a delayed status snapshot.

Current verification includes actual macOS PTY input, paste, pause and detach; a real DeepSeek task
received a Chinese correction through the TUI and completed after the client detached. Records are
in `artifacts/tui-live.ansi` and `artifacts/campaign/terminal-probe.json`. Computer Use prohibited
access to Apple's Terminal app, so native-window visual inspection remains unverified.
