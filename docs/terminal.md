# Using the Logit- Harness terminal

The executable is `loh`. In a development checkout, build with `cargo build` and run
`./target/debug/loh run "任务描述"`. Export `DEEPSEEK_API_KEY` in the launching environment.
Configuration and session state default to `~/.loh`, configurable with `LOH_HOME` or `--home`.

Running `loh` alone now opens a complete welcome screen with the current model, project directory,
task editor and recent sessions. It remains usable without an API key: commands and saved
conversations can be inspected, while starting/resuming model work requires the key. Missing-key
submission retains the draft and does not create a task or issue a provider request.

The interface occupies the normal terminal buffer rather than switching to an alternate screen.
Earlier shell output stays in native scrollback. The welcome screen is compact; an attached
conversation uses available height. Cursor position is queried only before asynchronous input
starts, avoiding the cursor-query/input-reader race during welcome-to-session transitions.

When stdin and stdout are terminals, `run` and `attach` open the Ratatui interface. With `--json`,
they stream structured messages. Redirected non-JSON output remains readable text. `run --detach`
starts the owned background daemon and returns its session ID. `loh sessions` lists saved work;
`loh attach <id>` reconnects. If a daemon has exited, `loh resume <id>` reconciles it before work
continues. Only one runner owns a workspace at a time.

Type a correction or question at any time. Enter submits; Alt+Enter inserts a newline. Pasted
multiple lines remain a draft until Enter. Editing respects grapheme boundaries for Chinese,
combining marks and joined emoji. PageUp/PageDown scroll the transcript.

Type `/` to open the command menu; continue typing to filter, use Up/Down to select, Tab to complete,
Enter to confirm, and Escape to close. Commands requiring arguments insert a trailing space and
wait for those arguments. Outside the menu, Up/Down move through multiline input and submitted
message history, retaining the unsent draft. Ctrl-D deletes at the cursor when the draft is nonempty;
with an empty draft it disconnects. Ctrl-U clears the draft.

Tool activity is grouped by action: command or path, streamed output, then a success/failure marker.
Normal output is collapsed; Ctrl-O expands it. Failed output shows its tail so the error is visible.
Interrupted actions with an unknown result never acquire a green success check. Responses support
headings, emphasis, inline code and fenced code. Model/tool text cannot execute terminal escapes.
The footer keeps token usage and the measured KV input hit rate visible; `/budget` has the details.

| Command | Behavior |
|---|---|
| `/status` | Read the current saved session state |
| `/sessions` | Select a saved task in this directory from welcome or a live conversation; switching clients does not stop the old task |
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

If the daemon is unavailable, `attach` opens the saved transcript without a model connection.
It supports `/status`, `/budget`, `/diff`, `/help`, output expansion and scrolling. `/resume` is an
explicit request to reconnect/recover; just reading a saved conversation cannot restart work.
Leave this saved-record view and reopen `loh` to choose a different session.

Current verification includes actual macOS PTY input, paste, pause and detach; a real DeepSeek task
received a Chinese correction through the TUI and completed after the client detached. Records are
in `artifacts/tui-live.ansi` and `artifacts/campaign/terminal-probe.json`. Computer Use prohibited
access to Apple's Terminal app, so native-window visual inspection remains unverified.

The subsequent CLI redesign was compared with locally installed Claude Code (unlogged-in UI) and
Codex. Its PTY captures, interaction receipt and validation logs are under `artifacts/terminal`.
These images render actual PTY character grids; they are not native Terminal-window screenshots.
See [the UI review](terminal-review.md) for the comparison and scope.
