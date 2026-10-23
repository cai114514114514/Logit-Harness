# Using the Logit- Harness terminal

The executable is `loh`. In a development checkout, build with `cargo build` and run
`./target/debug/loh run "任务描述"`. Configure credentials with `loh setup` or `/setup` in the UI.
Configuration and session state default to `~/.loh`, configurable with `LOH_HOME` or `--home`.

Running `loh` alone prints the blue L, current model and project directory once, then opens a
compact welcome editor; `/sessions` opens saved conversations. The brand is never redrawn between replies or tools,
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
message history, retaining the unsent draft and its cursor. Automatic line wrapping counts as
editor rows: an arrow only enters history after reaching the first/last displayed row. Once a
command has a space/arguments, its menu closes so it no longer captures editing arrows.
Ctrl-D deletes at the cursor when the draft is nonempty; with an empty draft it disconnects.
Ctrl-U clears the draft. Ctrl-A/Home and Ctrl-E/End move within the current logical line;
Ctrl-W or Alt-Backspace deletes the previous word, Ctrl-K clears the rest of the line, and
Alt-Left/Right or Alt-B/F moves by word. Input changes repaint immediately; streaming tool output
is still batched to avoid redrawing for every output fragment.

The activity area keeps one primary action, with measured elapsed time and time since output.
An old draft, pending authorization/question, cancellation settlement or workspace wait takes
priority over ordinary progress. If the available rows cannot contain an entire actionable command,
`/status` provides the complete list; a partial authorization ID is never presented as a usable command.
Enter shows local sending feedback immediately. Directed requests remain visibly pending until
their durable terminal receipt; late acknowledgements cannot restart waiting or clear another view's
submission. An ordinary main-view acknowledgement proves persistence only, not application or completion.

Input prompts follow the saved session mode: continue a conversation, add read-only Plan context,
or correct an Engineering task. Tool activity is grouped by action; full source, identity and local
start time are in details. Shell output is literal;
read/search/patch results have their own formatting. A settled successful tool normally occupies one
line. Adjacent successful reads/searches from the same member, assignment, turn and response can be
grouped within one append, at most three rows; further steps remain in details. A running command
keeps up to four rows of live output. Failures keep the exit code and up to four error-tail rows;
long commands reserve space for that exit code. Full output is retained in history, search and copy.
Known routine diagnostics and successful delivery receipts stay in full history, while failures,
unknown outcomes and required actions stay visible. Question and approval commands retain exact IDs.
Interrupted actions with an unknown result never acquire
a success marker. Display text cannot execute terminal escapes; explicitly copied content retains
its original bytes. The footer shows the model, reported token usage and KV input hit rate; `/usage` and
`/budget` provide details. Context size estimates are separate from provider-reported token usage.
Main shows the team's total; a member shows only its own reported usage. Missing KV data is `—`.
Narrow windows first omit secondary shortcuts and token totals, preserving identity, control scope,
the editor and actionable status. Supported layout checks include 110×34, 80×24, 60×18 and 40×12;
smaller unusable regions explicitly ask for more space.

When a team exists, its title and one stable member strip sit at the top of the active region,
below native scrollback. Ordinary chat has no roster. The selected member remains visible;
overflow is `+N`, and Ctrl-T opens the full picker. A member's header retains its task and
“仅控制此成员”; archived views retain the original team and “归档只读”. The strip does not reorder
on progress updates. Main also reports member wait reasons when there is no actual action running.
The `/` menu shows source and execution timing for the selected command, without repeating those
tags on every row. Existing command parsing, source-qualified Skills and control scopes are unchanged.

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

## Working with a team

An engineering goal uses one background session and one shared budget. Main assigns independent
work when useful; ordinary conversation remains a single agent. Members have persistent identities
and their own task context and messages. Opening another view does not create another agent or
model request. The default allows two executing members; waiting members retain task ownership
without occupying a model request slot.

| Command | Scope and behavior |
|---|---|
| `/team` | Inspect the team and select a member |
| `/agent <name-or-ID>` | View that member in the current terminal |
| `/agent main` | Return to main |
| `/agent <name-or-ID> --window` | Explicitly open a macOS Terminal window; on failure, show an attach command |
| `/tell <name-or-ID> <text>` | Send literal text to a member with a durable delivery receipt |
| `/team pause`, `/team resume`, `/team stop` | Control the entire team from any view |
| `/team limit <count>` | Set executing-member concurrency; zero prevents new dispatches without cancelling existing work |
| `/tasks` | Include task owners, dependencies and blocking reasons |

From another terminal, use `loh attach <session> --agent <name-or-ID>`. If the session uses a
nondefault state directory, also pass `--home <directory>`. The saved session determines the
workspace; the attaching terminal's working directory does not change it. Automatic delegation
never opens windows. Closing a member window or `/detach` only disconnects that client.

Use `/team`, or Ctrl-T while retaining a draft, to open the team picker. Up/Down selects,
Enter focuses the member, `o` requests a new window, and Escape returns. Switching views adds
a labeled boundary. Earlier views remain in the terminal's
native scrollback; new output and the history browser follow the selected view. Member views
retain separate drafts, and main summarizes team milestones while member views retain detailed
tool output. Status shows actual activity and waiting reasons rather than estimated completion.
Main and member headers keep the same team ID visible. Archived member views retain their
original team's identity even when the session has started another goal.

In main, Ctrl-C and `/pause`, `/resume`, `/stop` control the whole team. In a member view they
control only that member. A team resume preserves explicit member pauses; a member resume cannot
override a team pause or stop. Stopping a member first settles its running operations, then releases
its assignment. A later resume cannot reclaim a task already assigned elsewhere.

History, copying and compaction follow the selected view. Goal, mode, budget and permission
changes belong in main. Member instructions retain the expected assignment and version. A draft
from an old assignment is rejected with its text preserved rather than delivered to a new task.
Delivery/acceptance changes are recorded as requirement revisions; a member cannot expand its own
write scope through a message or Skill. Skills on active assignments retain the assignment's
existing capabilities after current operations settle; idle/archived members and explicit review
workflows use read-only contexts.

An accepted request means it is durably queued, not that its instruction has taken effect. The
later receipt distinguishes pending, applied and failed. Ordinary connection retries keep the
same request ID. If the daemon crashes midway through applying a directive, recovery preserves
the original text and any known effects and reports uncertain application. Engineering intent
that still needs reconciliation creates a scoped question; answer its exact ID from main with
`/answer <question-id> <text>`. Do not infer success from receipt acceptance or blindly repeat
possibly applied instructions.

Current clients require a matching IPC protocol. A mismatch reports an actionable error and does
not terminate the old daemon. An already connected matching client, or a preserved older binary,
can still control that daemon. Reading saved history does not start model work.

This local delivery keeps the pre-team executable at
`artifacts/team-2026-10-23/previous/loh` for existing older sessions. New sessions
use `/opt/homebrew/bin/loh`. The installation does not restart old daemons.
Consecutive member Skills retain arrival order and wait for the admitted model
response and its tools to settle. Their selected text remains in member history;
an Applied receipt is not a workflow-completion verdict. A paused existing
read-only reply stays suspended; a newly requested side question is distinct.

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
Historical development captures are in `artifacts/terminal-layout` and
`artifacts/terminal-layout-integrated`; delivery evidence is under `artifacts/interaction-delivery`.
Native terminal reflow remains owned by the terminal;
opening a taller menu can move settled rows into scrollback. Closing it clears only the old
active area and leaves reusable space above the bottom input; later output fills that space.
Repeated menu/editor toggles do not add blank rows to completed records or separate their
headings from their text.

The PTY scripts require Python with `pexpect`, `pyte` and Pillow on macOS. For an isolated
test environment, install those packages in a virtual environment, then run:

```sh
python tests/pty/layout.py --binary /opt/homebrew/bin/loh --output artifacts/layout-check
python tests/pty/interaction.py --binary /opt/homebrew/bin/loh --output artifacts/interaction-check
```

Both scripts use temporary state/workspace directories and a localhost provider. They do not use
the user's credentials, clipboard, sessions or paid model allowance. The renderer comes from
`tests/pty/grid.py`; screenshots are captured character grids, not native-window acceptance.

The team delivery adds `tests/pty/team.py`. Its installed-binary records are under
`artifacts/team-2026-10-23/installed/team`, including event JSON, ANSI transcripts,
request receipts and grids. `/team`, focus changes and attach/history operations
are checked for zero additional provider requests while the fixture is idle.
For manual native-window QA, run `python tests/pty/team.py --binary
/opt/homebrew/bin/loh --serve` in an environment with the same Python dependencies.
It prints three isolated attach commands and stays alive until Ctrl-C cleans up
only that fixture. It is explicitly offline and never opens native windows itself.
See [team acceptance](team-acceptance.md) for the remaining manual checks.
