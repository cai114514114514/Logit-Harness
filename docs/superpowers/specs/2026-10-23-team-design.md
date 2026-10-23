# loh multi-window team design

User-approved design, 2026-10-23. One engineering goal, one team, one daemon,
workspace lease and shared budget. Keep Rust, DeepSeek Flash, blue L, native
terminal scrollback and requirement-aware corrections.

## Team behavior

- Main owns requirements, dependency planning and independent acceptance. It
  automatically delegates useful ready work, at most two executing members by
  default. Greetings, knowledge chat and opening a view never create workers.
- Members have stable identities, independent contexts, assignments with
  versions, and persistent addressed mail. A new execution attempt gets a new
  Actor; a completed Actor is never revived. Idle members can be reused.
- Members can send messages to peers/main and wait for a reply. Messages are
  data, not authority to expand scope, change goals, claim tasks or spawn agents.
  Only main delegates/reassigns. Each task has a single live owner.
- Reads/model work can overlap. Guarded patches and checks retain the existing
  mutation lock. Assignments declare writable files/directories; proposal
  application checks ownership, assignment/step/requirement versions, paths and
  hashes. Conflicts return useful failure information for a fresh read/repair.
- A local execution correction records the user's exact words and invalidates
  its step, dependents, assignments, actions and evidence. Any delivery or
  acceptance change enters the canonical requirement revision path. Unclear
  impact is conservative. Cancelled writes must settle before replacement.
- User side questions are read-only and do not replace/pause the engineering
  objective. Directed messages cannot leak into every actor's user inbox.
- Waiting or paused members retain ownership but do not occupy model request
  capacity. When nothing is runnable, detect peer wait cycles/impossible waits
  and notify main once per changed wait graph. Do not poll the model while
  waiting for actual commands, user input or valid pending delivery.
- Completion requires settled engineering assignments, current evidence and no
  unprocessed corrections/handoffs/work-changing messages. Idle members and
  read-only side questions do not block. The final control-watermark check and
  Completed state/event must commit in ONE SQLite write transaction.

## User interface

Main retains its conversation plus compact member rows: name, task, real action,
waiting reason/source and last progress age. Main history contains milestones;
detailed member tool streams stay in member views. Member views always identify
team/member/task/control scope and show their own chat/tools/mail/usage. Record
assignment boundaries. Drafts bind to their original assignment.

- `/team`: navigable roster/tasks/mail; select/view/open member.
- `/agent NAME`: focus member here; `/agent main`: return to main.
- `/agent NAME --window`: explicitly launch macOS Terminal attached to member.
- `/tell NAME TEXT`: durable addressed user input with receipt status.
- `/team pause|resume|stop`: control whole team in any view.
- `/team limit N`: current member execution limit; default 2, zero prevents new
  member dispatch; reducing the limit does not kill existing operations.
- `loh attach SESSION --agent MEMBER`: manual connection from any terminal.
- `/tasks`: existing workers/commands/services plus owner/dependency information.
- Ctrl-C and `/pause`, `/resume`, `/stop` target the member in a member view and
  the whole session in main. Global resume does not override local pause;
  local resume cannot override global pause/stop. Member stop settles operations
  before releasing its task. Resume never steals a reassigned task.
- `/history`, `/copy`, `/compact` target the focused view. Changes to budget,
  permissions, goals and execution mode require the main view. Skill invocation
  remains target-scoped, cannot enlarge authority. `/new` starts another chat
  while the old team runs. Closing windows only detaches.
- Auto-delegation never auto-opens windows. Launch only on user action, quote
  trusted executable/home/session/member arguments, never interpolate user text
  or secrets; show a copyable attach command if launching fails.
- Keep append-once scrollback, recent 20-block/200-line initial recovery and
  complete searchable history. Preserve CJK/multiline editing and resizing.

## Persistence, transport and resource boundaries

Add stable Member/Assignment/Mail types and lightweight public TeamView/MemberView.
Directed controls capture client/request ID, target and expected assignment;
reuse Store.enqueue_once. Ack means persisted, not completed. Track pending,
applied and failed receipts; retries deduplicate. Stale target rejects visibly
and preserves input. Process actual controls in server order.

Keep one global event sequence; correlate ownership before projecting into views.
Filtered events still advance consumed sequence. Histories, drafts and late
asynchronous replies remain target-bound. Stream public views, not model contexts.
Recovery reconciles processes/files before redispatch, never blindly replaying
unknown writes. Preserve old saved sessions and events. Reject incompatible IPC
with actionable version information; never kill a user's old daemon to upgrade.

All roles use the configured DeepSeek Flash and existing shared token/time/money
budget, including steering's reserved request lane. Usage/KV comes from actual
provider usage and is counted once. Keep tool ordering/system prefixes stable;
append addressed state/mail at the tail. Compression preserves current constraints,
assignments, pending mail and tool pairs. No worktrees/plugin runtime/terminal
emulator. Do not alter frozen research or reset the 50 RMB ledger. No paid tests
without sufficient existing shared allowance.

## Acceptance

Exercise one main/two members, peer exchange/dependencies, scoped/global
correction, stale/overlapping patches and user edits, local/global pause/stop,
wait cycles, unique assignment, three simultaneous clients, retries/reconnect,
stale drafts, crashes, budget boundaries and final-commit/control races.
Run complete regression and actual macOS three-window Terminal opening,
closing, reattaching and local-control checks. Report engineering tests,
real-window checks and live-model behavior separately. Offline fixtures cannot
prove real Flash teamwork quality.
