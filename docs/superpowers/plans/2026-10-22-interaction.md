# loh Interaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkboxes for tracking.

**Goal:** Deliver the approved lightweight conversational runtime, command catalog and native terminal history.

**Architecture:** Keep the existing Rust runner and tools. Add explicit conversational/task state and durable goal identity, then a command service shared by interactive clients. Derive terminal views from correlated events and append settled blocks to native scrollback.

**Tech Stack:** Existing Rust, Tokio, SQLite WAL, Ratatui/Crossterm, reqwest and MCP SDK; no new plugin runtime.

**Spec:** `docs/superpowers/specs/2026-10-22-interaction-design.md`

## Global Constraints

- Work in the original tree, no worktrees, no sed, no V2/next naming.
- Readable responsibility-focused modules; preserve unrelated edits and frozen research artifacts.
- Official deepseek-flash; 50 RMB total existing campaign allowance; never expose credentials.
- Model final text does not establish engineering completion.
- User already approved the full design and requested execution; proceed without another approval round.

## Review Focus

- A greeting must avoid filesystem discovery before the first model call as well as avoid tools afterwards.
- Late results from another goal must not authorize writes or verification in the current goal.
- A new chat must remain usable while another daemon owns the workspace.
- Literal terminal logs containing Markdown or wide text must survive resize and replay without reinterpretation or duplication.
- Skills and local commands must not elevate permissions, escape plan mode or contaminate cached model input.

## Task 1: Conversation and execution boundaries

**Files:** runtime state/actor/runner/dispatch/observe/control/daemon, focused new runtime modules for conversation and ownership; domain goal/event; extensions knowledge; tests/conversation.rs and existing runtime tests.

**Interfaces:** Preserve `Session::new` as an explicit engineering constructor for evaluations/tests; add `Session::conversation(workspace, input, tokens, seconds)` for ordinary CLI input. Expose `Control::Plan { text: Option<String> }`, `Control::Goal { text: String }`, `Control::Side { text: String }`, `Control::Compact`. Add durable stable goal/response/turn identities without losing readable existing sessions. Document the final fields in the task report for CLI consumers.

- [ ] Add failing fixture-provider tests: greeting finishes one response with no tools, goal, verifier or project discovery; a project question can read but not mutate; an explicit task enters the existing engineering loop.
- [ ] Run `cargo test --locked --test conversation`; capture the initial failure.
- [ ] Implement conversational routing in the first response, lazy knowledge, sticky plan mode, archived independent goals/persistent queued tasks, epoch/goal guards and scoped workspace ownership. New chats wait only when promoting engineering work.
- [ ] Add tests for plan escape prevention, late results, multiple goals, pending work, two daemons, ownership handover and crash recovery. Keep explicit evaluation sessions unchanged in behavior.
- [ ] Run focused runtime/provider/steering/cache/recovery tests; commit this independently usable runtime change.

## Task 2: Command services and Markdown workflows

**Files:** new src/commands modules; config settings persistence; extensions Skills/MCP; runtime control adapters; tests/commands.rs; bundled workflow SKILL.md resources.

**Interfaces:** `commands::parse(&str) -> Result<Command>` and metadata directory shared by parser/help/completion; provide a non-TUI command service taking Paths, workspace and optional session id, returning typed local output, runtime Control, Skill invocation or session-navigation intent. Expose these exact public APIs in the report before terminal integration.

- [ ] Add failing tests for every approved command family, unknown arguments, core-name collision and duplicate Skill source selection.
- [ ] Implement the complete spec command list; keep existing command semantics. Configuration is nonsecret/default-only, permissions reuse exact-action approval, task list is observational, MCP controls preserve uncertain outcomes.
- [ ] Add bundled /init and /review, literal Skill arguments and lazy source-aware discovery. Preserve project guidance, plan mode and permissions.
- [ ] Test local commands without a provider, config/budget preservation, MCP safe-boundary updates, explicit Skill invocation and no shell substitution.
- [ ] Run `cargo test --locked --test commands` and affected extension tests; commit.

## Task 3: Terminal history and command integration

**Files:** cli/terminal focused modules; CLI launch/navigation; store pagination; tests/terminal*.rs and tests/transcript.rs.

**Interfaces:** Reuse command services from Task 2 and correlated runtime events from Task 1. Separate transcript projection, settled-block output, active composer and history browser; keep default normal terminal buffer.

- [ ] Add failing projection and PTY regressions for raw tool output, late/interleaved events, no duplicate settlements and history older than 300 entries.
- [ ] Append settled blocks once with source and start time; redraw only active area. Provide native scrollback, stable resize/reconnect and a recent-context replay of 20 blocks capped at 200 lines.
- [ ] Implement temporary paginated/searchable history with per-entry expansion/copy, wire Ctrl+O and /history, and integrate all command families in welcome/live/archive states.
- [ ] Implement the user's added visible-activity requirement: authoritative current model/tool/worker/verifier state, measured duration/output age, distinct workspace/user/approval waits and actionable owner navigation. Settle the indicator on actual completion, including tool-only model responses; do not retain the last request label.
- [ ] Keep Enter startup/connection failures visible with recoverable draft/session state, without silently retrying possibly delivered requests.
- [ ] Test Chinese input, paste, resize, /setup masking, navigation to a new chat without stopping old work, local commands during active tasks and close/detach behavior.
- [ ] Run terminal/projection/CLI tests and actual PTY capture; inspect the rendered grids; commit.

## Task 4: Whole-delivery validation

**Files:** docs/progress.md, README.md, docs/terminal.md, relevant acceptance documents; artifacts/interaction and a new package directory.

- [ ] Run `cargo fmt --all --check`, `cargo test --locked --all-targets`, and `cargo clippy --locked --all-targets -- -D warnings`; fix material regressions and retain logs.
- [ ] Review all changes against the spec with an independent fresh reviewer; address load-bearing findings and run affected checks.
- [ ] Inspect the existing campaign account without changing uncertain charges; only perform bounded live Flash probes if conservative reservations fit its 50 RMB limit. Record actual behavior separately from fixture results.
- [ ] Build locked macOS release, verify signature, atomically install loh, rerun installed-binary PTY checks and package to a new directory without credentials.
- [ ] Update progress/docs with exact test and live-provider evidence; preserve earlier packages and the frozen research executable; commit final delivery.
