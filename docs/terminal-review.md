# Terminal interaction review — 2026-10-22

The user rejected the earlier CLI and asked us to inspect locally installed Claude Code and Codex.
This is a refinement of the existing Rust client, with the runner, model protocol, permissions,
correction mechanism and shared paid budget left in place.

## Direct local observations

- Claude Code 2.1.304: entered its actual interactive PTY in this project. The welcome screen showed
  the product/version, model, working directory, ruled composer, shortcuts, and “Not logged in”.
  Typing `/` opened a selectable command catalog with descriptions. No login was attempted and
  no development task was submitted. The inspected client was closed afterwards.
- Codex CLI 0.159.3: inspected the local interactive CLI with `--no-daemon --no-alt-screen`, including
  its compact header, composer, model/path footer and filtered slash-command list. No task was
  submitted. The separate inspected client was closed; the user's existing sessions were retained.
- The previous `loh` began with `read_line("任务 ›")`, then switched into a different full-screen
  editor. Tool records said only “execute tool”/“tool successful”, losing the actionable file,
  command and failure output. There was no discoverable command palette or input recall.

## Changes

One editor now handles both starting and steering a task. The welcome screen includes model,
directory, credential state and recent project sessions. Normal terminal-buffer rendering preserves
earlier shell scrollback. The composer uses horizontal rules, leaving conversation and tool output
visually distinct without heavy panel borders. Blue marks interaction/progress after the user's
L-icon branding update; success/error
markers carry meaning independent of color. `NO_COLOR` is respected by the terminal backend.

The command catalog supplies help, filtering, completion and keyboard selection. History navigation
retains unsent drafts; multiline paste still requires Enter. Tool output stays with its action even
while other actors produce messages. Completed streams replace their pending entry rather than
duplicating it. Output is collapsed with explicit expansion; failed commands show their error tail.
Basic Markdown and code rendering preserve readable structure. The KV metric remains provider-
reported and uses the previously verified weighted-input denominator.

Saved transcripts can be opened without a daemon or key. Reading does not resume a task; `/resume`
does. The live session picker changes only the connected client, leaving background execution alone.
Source modules separate editor, menu, transcript, text layout, welcome, archive, surface ownership,
socket transport and rendering.

## Verification

- Added regressions for command completion, argument entry, Unicode multiline navigation, draft
  recovery, action/output ordering, visible failures, interrupted-result presentation and resize.
- Actual PTY driver exercised missing-key startup, slash menu, Tab completion, multiline paste,
  100-column to 54-column resize, recent-session selection, archive viewing, expanded output,
  KV footer, rejected keyless resume and exit. It checked the saved state was not modified.
- The welcome-to-archive path exposed a real cursor-query timeout missed by rendering tests.
  Surface ownership now records its ending cursor row and avoids querying stdin after the async
  input reader has been created. Repeating the same full PTY path passes.
- Existing real PTY coverage still checks corrections, pause, detach and a continuing socket server.
  The complete 143-test suite and strict all-target Clippy pass. Results are in
  `artifacts/terminal/tests.log` and `clippy.log`.

`artifacts/terminal/interaction.json` records the actual PTY checks. `welcome.png`, `commands.png`,
`narrow.png` and `conversation.png` are images rendered from actual character grids. Font rendering
is supplied by the capture helper, not Apple's Terminal app; native-window visual inspection remains
unverified. The conversation demo is explicitly synthetic (`examples/terminal_preview.rs`) and is
not presented as a new model-quality or cache-hit experiment. No paid provider request was made.
