# Using the Logit- Harness terminal

The executable is `loh`. In a development checkout, build with `cargo build` and run
`./target/debug/loh run "任务描述"`. Configure credentials with `loh setup` or `/setup` in the UI.
Configuration and session state default to `~/.loh`, configurable with `LOH_HOME` or `--home`.

Running `loh` alone prints the blue L, current model and project directory once, then opens a
compact welcome editor and recent sessions. The brand is never redrawn between replies or tools,
including after `/new`, session switching, resize or reconnect. It remains usable without an API key: commands and saved
conversations can be inspected, while starting/resuming model work requires the key. Missing-key
submission retains the draft and does not create a task or issue a provider request.

The main interface occupies the normal terminal buffer. Completed responses and tool results
are appended once to native scrollback; only active work and the composer are redrawn. The
conversation editor and usage footer stay at the physical terminal bottom when running or idle.
The most recent response sits above the measured activity and input. `/new` keeps that bottom
layout without reopening welcome copy. Resizing or reconnecting does not replay completed output.
Cursor position is queried only before asynchronous input starts. A new client shows up to 20
settled blocks / 200 wrapped lines with a recovery marker; the rest remains in `/history`.
Your terminal controls its own scrolling and reflow; cross-terminal scroll-position preservation
is not guaranteed.

When stdin and stdout are terminals, `run` and `attach` open the Ratatui interface. With `--json`,
they stream structured messages. Redirected non-JSON output remains readable text. `run --detach`
starts the owned background daemon and returns its session ID. `loh sessions` lists saved work;
`loh attach <id>` reconnects. If a daemon has exited, `loh resume <id>` reconciles it before work
continues. Only one engineering session owns source execution in a workspace at a time. Ordinary
conversations can coexist. A second engineering session waits with the owner's title/ID visible,
without starting its engineering model or workers. Source changes must settle before handover.

A greeting or general knowledge question is an ordinary conversation, without project discovery
or an engineering goal. Project questions can use read-only tools. An explicit development request
starts engineering execution and independent acceptance. `/plan` stays read-only until you
explicitly authorize implementation; Skills cannot override it. `/new` (also `/clear`) opens a new
conversation while the previous task continues in the background. `/sessions` switches clients;
`/resume` resumes execution. These commands do not delete old records.

Type a correction or question at any time. Enter submits; Alt+Enter inserts a newline. Pasted
multiple lines remain a draft until Enter. Editing respects grapheme boundaries for Chinese,
combining marks and joined emoji. Use native terminal scrolling for completed output; PageUp,
Ctrl+O or `/history` opens a temporary history browser.

Type `/` to open the command menu; continue typing to filter, use Up/Down to select, Tab to complete,
Enter to confirm, and Escape to close. Commands requiring arguments insert a trailing space and
wait for those arguments. Outside the menu, Up/Down move through multiline input and submitted
message history, retaining the unsent draft. Ctrl-D deletes at the cursor when the draft is nonempty;
with an empty draft it disconnects. Ctrl-U clears the draft.

The persistent activity area shows actual outstanding model requests, commands, workers and
verification, plus measured elapsed time and time since output. Workspace wait, pending questions,
approval, cancellation settlement, pause and completion have distinct labels and relevant commands.
It does not infer progress from the last model-request text or display invented percentages.

Input prompts follow the saved session mode: continue a conversation, add read-only Plan context,
or correct an Engineering task. Tool activity is grouped by action; full source, identity and local
start time are in details. Shell output is literal;
read/search/patch results have their own formatting. Normal results show a summary; failures retain
the useful tail and folding is labeled. Known routine diagnostics stay in full history, while failures,
unknown outcomes and required actions stay visible. Question and approval commands retain exact IDs.
Interrupted actions with an unknown result never acquire
a success marker. Display text cannot execute terminal escapes; explicitly copied content retains
its original bytes. The footer shows reported token usage and KV input hit rate; `/usage` and
`/budget` provide details. Context size estimates are separate from provider-reported token usage.

The history browser reads the full durable event record, including entries older than 300 events.
Use Up/Down to select, PageUp/PageDown for pages, Enter to expand one entry, `j`/`k` to scroll its
body, `/` to search and `c` to copy its original content. Escape or `q` returns to the normal buffer.
`/copy` opens selection; `/copy <unique-entry-ID-prefix>` copies a specific entry. Loading uses
SQLite pages off the input loop, but the resulting projection occupies memory proportional to the
history size. Clipboard operations use macOS `pbcopy` and report failures.

| Command | Behavior |
|---|---|
| `/setup` | Configure the API Base URL and hidden API Key locally |
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
| `/new`, `/clear` | Start a new conversation; old records and background work remain |
| `/rename <title>` | Rename the current conversation |
| `/export <path>` | Export Markdown, or structured events with an explicit `.json` path; refuse overwriting |
| `/context`, `/usage` | Inspect context sources/size estimates, or reported usage and KV hits |
| `/compact` | Compress history after outstanding calls settle, preserving requirements and tool pairs |
| `/plan [text]` | Enter read-only planning; bare `/plan` on welcome makes no model request |
| `/goal [text]` | Inspect the goal, or explicitly submit a goal/requirement change |
| `/tasks [ID]` | Inspect workers, commands, services and pending work |
| `/btw <question>` | Ask a read-only side question while retaining the current goal |
| `/history [query]` | Search/browse the complete record; `/history tools [query]` filters tool calls |
| `/tools [name-or-source]` | Inspect capabilities, prerequisites and permission boundaries locally |
| `/copy [ID]` | Select content to copy, or copy a specific entry by unique ID prefix |
| `/skills`, `/skills run <name-or-ID> [args]` | Refresh/browse Skill metadata or explicitly invoke a source |
| `/<skill-name> [args]` | Invoke a Markdown workflow with literal arguments; collisions choose a source |
| `/init`, `/review` | Bundled initialization and read-only review workflows |
| `/mcp` | Inspect configured services; `inspect`, `reconnect`, `enable`, `disable` take a service name |
| `/config [key [value]]` | Inspect/edit nonsecret defaults for new conversations, not current execution |
| `/permissions` | Inspect policy and exact pending authorizations |
| `/doctor` | Local environment and credential-source diagnosis; no provider call |

The command menu and `/help` share the parser's directory and show source and timing. Local
inspection stays out of model context. Runtime controls target their originating session;
settlement controls wait for active operations. Skill bodies load only on invocation. Core control
names remain reserved, and explicit `/skills run <ID>` disambiguates a conflicting name.

`/config` takes effect with `/new` or a new `loh run`; existing conversations retain their initialized
model/runtime defaults, including after restart. Use `/budget` for the current limit and `/setup`
for credentials. MCP changes take effect after calls settle, retaining uncertain external outcomes.

`/setup` works in the welcome screen, live conversations and saved-record view. `loh setup`
opens the same form directly. Tab or Up/Down switches fields; Enter advances from the URL to the
key, then saves. Ctrl-U clears the selected field and Escape cancels without saving. Keys are
masked and the form is separate from chat history and persisted session events. Inline arguments
such as `/setup <key>` are rejected; paste the key only into the hidden field.

The default endpoint is `https://api.deepseek.com`. Enter a Base URL, without `/responses`;
HTTPS is required except for localhost testing. Saved keys are bound to the complete normalized
URL, including its path. A blank key retains credentials already configured for that endpoint.
Nonempty `DEEPSEEK_API_KEY` takes precedence, and the form labels that override.

The key is stored separately in `~/.loh/credentials.json` (or the selected state home), with file
mode 600 inside a mode-700 directory. This is permission-protected plaintext, not encryption or
macOS Keychain. The ordinary `config.toml` contains no key; unrelated settings are preserved.
Saving performs local validation only, with no model request. New daemons read the saved key;
an already-running daemon keeps its initialized provider until restarted. `doctor --json` reports
credential presence and source, never its value.

An authorization request displays the concrete tool, redacted arguments, current origin where
applicable, and a private JSON review-file path for the full preview. The ID binds the original
action; approving it queues that action for a fresh scope check. A changed origin requires a new
approval. Known credential fields and common credential forms are redacted in the preview.

Closing the terminal only disconnects the client. Controls become durable when the socket server
acknowledges them. If delivery is unconfirmed, the interface restores message text where possible;
inspect the session before resubmitting to avoid duplicates. The display does not rewind its state
when replaying older events or receiving a delayed status snapshot.

If the daemon is unavailable, `attach` opens the saved transcript without a model connection.
The same command service works there, including setup, inspection, full history, export and session
navigation. `/resume` explicitly reconnects/recovers; merely reading records cannot restart work.
Startup failures retain their reason, saved session and draft. Possibly delivered inputs are not
automatically resent: inspect `/status` or `/history` before retrying them.

Current verification includes actual macOS PTY input, paste, pause and detach; a real DeepSeek task
received a Chinese correction through the TUI and completed after the client detached. Records are
in `artifacts/tui-live.ansi` and `artifacts/campaign/terminal-probe.json`. Computer Use prohibited
access to Apple's Terminal app, so native-window visual inspection remains unverified.

The subsequent CLI redesign was compared with locally installed Claude Code (unlogged-in UI) and
Codex. Its PTY captures, interaction receipt and validation logs are under `artifacts/terminal`.
These images render actual PTY character grids; they are not native Terminal-window screenshots.
See [the UI review](terminal-review.md) for the comparison and scope.

The subsequent conversation-layout correction is exercised by `tests/pty/layout.py` and
`tests/pty/interaction.py`: bottom docking, one startup brand, once-only settled scrollback,
mode prompts, CJK editing, menu/detail return, resizing, setup masking and cancellation.
Current colored grid captures and machine-readable receipts are in `artifacts/terminal-layout`
and `artifacts/terminal-layout-integrated`. Native terminal reflow remains owned by the terminal;
opening a taller menu can move settled rows into scrollback. Closing it clears only the old
active area and leaves reusable space above the bottom input; later output fills that space.
Repeated menu/editor toggles do not add blank rows to completed records or separate their
headings from their text.
