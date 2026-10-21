# Logit- Harness implementation ledger

Plan: docs/superpowers/plans/2026-10-21-weft.md
Spec: docs/spec.md

## Decisions

- User explicitly approved implementation, so proceed without a second plan-approval interruption.
- Native execution with a fresh final review. All changes stay in the original directory as requested.
- A repository was initialized inside residual to avoid committing the unrelated parent home repository.
- User authorized 50 RMB total real evaluation. DEEPSEEK_API_KEY was absent at initial inspection;
  user replied OK about configuration, so verify actual availability before paid calls.
- Shared interfaces: domain -> provider/tools -> runtime -> CLI/evaluation. Typed public contracts
  live in the relevant modules and are verified through integration tests, not duplicated definitions.

## Status

- Task 1: complete — watched goal tests fail (2/3), then evidence/budget/store tests fail (6/9).
  `cargo test` passed 9/9 integration tests. SQLite reopen, stale evidence, local requirement updates,
  omitted criteria and budget reservation behavior are exercised.
- Ruling: use `Goal::missing_evidence` rather than a boolean `can_finish`; the runtime needs the
  missing requirement IDs to continue useful work. Cost if wrong: interface rename only.
- Task 2: provider/context core complete — local HTTP transport and fragmented SSE exercised;
  watched four transport/context tests fail, then malformed-call identity regression fail.
  `cargo test`: 16/16 passed. Real DeepSeek calls remain unperformed.
- Ruling: rate-limit retries happen in transport; potentially billable 5xx retries require a new
  runtime budget reservation rather than hiding several uncertain calls behind one allowance.
  Cost if wrong: fewer automatic retries, not unaccounted spend.
- Task 3 core: hash-guarded create/replace/delete, real PTY process groups, source search,
  tool dispatch authorization and requirement leases, web HTTP transport/limits implemented.
  Watched 7 base tool tests fail, then stale-action/service/search/web tests fail before implementation.
  `cargo test` passed 28/28. macOS sandbox-exec actually prevented source writes while allowing build output.
- Pending integration: long-lived service lifecycle, persisted quota restoration, runtime metering,
  cancellation/reconciliation and compaction remain in Task 4; don't claim a functioning agent yet.
- Official price snapshot saved in docs/pricing.md; peak-rate upper bounds will enforce the 50 RMB
  campaign cap. No paid model or search call has been made. Environment key remains absent.
- Task 4 in progress: persistent session/actor state, revision impact DAG, cross-process nano-RMB
  reservations, main/verifier/steering actor loop, input classification, actual cancellation and
  independent evidence-backed completion implemented. These are library capabilities, not yet a CLI.
- Runtime tests prove real file creation, unsupported completion withheld, correction cancelling
  a real PTY command, question-after-completion state restoration, and malformed plan recovery.
- Regressions found through failing tests and fixed: own patches refresh the repository fingerprint;
  search absence evidence is rooted in a repository hash; deleted-file evidence expires on recreation.
  Latest full `cargo test` passed 39 tests (10 domain, 2 money, 7 provider, 5 runner, 2 graph,
  11 tools, 2 web). No real model call has been made.
- Remaining Task 4 work: money ledger wiring into actual inference/web calls; scoped parallel workers,
  requirement decomposition and step leases; progress/stagnation control; durable recovery and Unix
  socket daemon; long-lived service manager; dependency-specific questions instead of blanket waiting.
- Next immediate milestone: configuration + local daemon/CLI integration, followed by workers and
  extensions. CLI entrypoint is needed to exercise real child-process detach/reconnect/restart.

## Completed checkpoints and current work

- CLI/daemon checkpoint: basic run/attach/pause/resume/stop/doctor, durable Unix-socket controls,
  detached child process, private state paths and cross-process workspace ownership are implemented.
  Targeted CLI (2), daemon (1), IPC (3), money (4), and web (2) tests passed.
- Campaign wiring now shares a durable 50 RMB account across inference roles and search/fetch.
  Credentials were found in the user-provided ignored `.env` as `ds_api`; validation maps that value
  into the child process's `DEEPSEEK_API_KEY` without printing or modifying the secret file.
- First real Flash smoke run 3827144166944bc9 created working Python and passed its assertions, but
  exposed unnecessary verifier/implementer back-and-forth: verifier lacked an executable check tool.
  Preserved all trace/artifacts under artifacts/campaign. Accounted peak-rate cost: 0.09839576 RMB.
  This is a functional smoke test, not evidence of quality improvement.
- Added source-readonly `check` for independent verification, with macOS sandbox enforcement and
  pre/post source freshness checks. Actual Python execution and denied source writes tested RED→GREEN.
  Second real smoke e33a8cf665504266 completed but revealed malformed verdict IDs being treated as
  implementation failures. Added atomic report validation and current-ID schema constraints; regression
  proved the verifier repairs its own report without additional implementation calls (RED→GREEN).
- Third real smoke 138c9cabdf2a4f05 completed, 4 main turns + 6 verifier turns, 21,171 reported tokens.
  Total accounted peak-rate cost for all three smokes: 0.19206920 RMB; no uncertain calls.
  All are debugging tasks outside the frozen evaluation corpus, not a controlled quality claim.
- Persistent transition intent now retains resume during cancellation; completed tasks do not reenter
  Running on a late resume (RED→GREEN). Kernel process identities and a journal-before-execution gate
  prevent PID reuse and unjournaled side effects. Recovery closes interrupted transcript calls,
  reconciles real processes/files, preserves user edits, and never automatically replays uncertain writes.
  Real daemon SIGKILL/restart tested. An observed completion/resume race was reproduced deterministically
  and fixed; process exits are confirmed before replacement execution.
- Money reservations now wait for concurrent requests to settle; dropped requests remain conservatively
  charged, and dead process owners are recovered as uncertain cost without resetting the campaign cap.
  `cargo test` now passes 59/59 integration tests, including the real detached daemon and crash recovery.
  Current remaining runtime work: scoped parallel workers and proposals, explicit requirement
  decomposition, dependency-specific questions, stagnation handling, durable web quotas and services.
  Browser/MCP/Skills, full interactive terminal, frozen quality experiments and release packaging remain.

- Worker checkpoint in progress: exact-quote requirement decomposition retains the original goal and
  revision history; dependency scopes include transitive step versions. Two workers run concurrently,
  propose patches through the central owner, and stale dependency leases reject otherwise-current files.
- Actual correction test: an old worker deliberately ignores cancellation and replies late; its patch
  is discarded while the unaffected interface worker keeps its context and completes. Parallel fixture
  also exposed a one-second timer delay between local steps; scheduling now continues immediately and
  drains controls between iterations (test improved from timeout at 1s to about 0.1s).
- Missing inputs now have requirement scopes. An interface worker finishes while storage awaits a
  decision, and the answer versions dependent requirements and resumes their work. Side questions can
  use a fresh read-only context, preserving the original goal and prior paused/completed state.
- Split runtime metadata, steering, questions, worker coordination and verification into separate files.
  Evidence now also records step versions. Full suite passed 65 tests at this checkpoint.
- Concurrent original-tree readability edits appeared during a turn interruption. Preserve them and
  review semantic changes; no bulk reset or overwrite. Corrected two unsupported comments (cached-token
  accounting and an unmeasured reasoning-quality claim) while retaining surrounding improvements.
- Stagnation now starts a fresh diagnosis context after repeated settled observations with no new
  information. A real-file fixture that kept reading a missing file switched to a guarded create and
  completed without user prompting. Running commands are not treated as stuck merely due to elapsed time.
- Main plus two busy model requests no longer starve steering: one reserved steering request lane is
  available, still charged to the same budgets. Saturated-concurrency correction test RED→GREEN.
- Unscoped requirement additions/replacements advance a conservative dependency epoch; old actions,
  workers and evidence expire together. Exact initial decomposition does not advance it, and scoped
  corrections keep unrelated work valid. Domain tests cover both behaviors.
- Managed source-readonly services now run independently, confirm loopback HTTP readiness, write only
  an owned `.weft-build` output directory, and stop on invalidated dependency scopes or session stop.
  Actual Python HTTP server test verifies denied source writes, allowed build output, simultaneous
  foreground source edits and confirmed server shutdown. Restored unbounded post-SIGKILL confirmation:
  requesting a signal cannot release source ownership before actual process exit.
- Browser uses Chromiumoxide 0.9.1 with an isolated Chrome profile, real click/Unicode insertion,
  DOM/AX, console/errors and PNG screenshot artifacts. Actual Chrome test passed; visually inspected
  artifacts/browser-proof/browser.png shows `你好 Weft` in the input and rendered result. Native image
  parts now reach Responses tool output. The metered real Flash vision probe correctly read the
  screenshot's Chinese input/result and Apply button (383 input + 110 output tokens). Report:
  artifacts/campaign/vision-probe.json. Cumulative accounted peak-rate spend is **0.19371520 RMB**,
  reserved/uncertain both zero. This is transport/vision verification, not a quality-comparison result.
- Latest full `cargo test`: **72/72** integration tests passed, including actual Chrome, HTTP server,
  detached daemon/SIGKILL recovery, saturated steering and stale-worker rejection. No holdout evaluation
  has run; the frozen corpus and all four experimental conditions still need implementation.

## Product naming

- User correction: product name **Logit- Harness**, command/crate **`loh`**. Current source, CLI,
  configuration (`LOH_HOME`, `~/.loh`), service output (`LOH_OUTPUT_DIR`, `.loh-build`), tests and
  user-facing docs use the new name. Historical artifacts and the `weft-first-evaluation` money
  account retain their original identifiers so the 50 RMB cap and audit trail remain continuous.

## Extensions checkpoint

- `loh --help` displays Logit- Harness; CLI, detached-session, recovery and service tests use the
  new binary name and paths. Current implementation and fixtures contain no old product identifiers.
- Official RMCP 3.6 now provides actual stdio child and Streamable HTTP calls, lazy schema discovery,
  locally configured effects, native image results and process ownership. Readonly workers cannot
  invoke a configured writer. Local cancellation confirms group exit before returning. Remote
  uncertain writes stay journaled across restart and require scoped reconciliation authorization.
  A regression test reproduced inspection accidentally clearing that record; corrected RED→GREEN.
- Skills metadata/body/resources and directory-scoped AGENTS.md are loaded with path/hash provenance.
  Tests cover project/user discovery, YAML folded descriptions, dependency exclusions, changed
  metadata, escaped/symlinked resources, subtree rules and guarded first edits in a new directory.
  Explicit user requirements remain above skill guidance; metadata cannot grant tool permissions.
- Ruling: use SDK discovery lifecycle `2026-07-28` without a custom legacy fallback; optional MCP
  task/input continuations are reported incomplete, never completed or blindly replayed. This keeps
  one implementation; servers limited to older handshakes or optional continuations need adjustment.
- Ruling: Rust minimum is 1.89 because the maintained YAML parser used for skill frontmatter requires
  it. Local Rust 1.95 builds it. Users with older Rust must update their compiler or use the binary.
- Full `cargo test`: **81/81** integration tests passed after rename and extension integration.
  This includes actual Chrome, both MCP transports and interrupted remote-mutation recovery.
  Campaign ledger remains **0.19371520 RMB** accounted, zero uncertain/reserved, 50 RMB cap unchanged.
  No additional paid provider calls were made in this checkpoint.

## Implementation watch list

- Study mechanisms are now real `Session.condition` switches, with dynamic as the normal default.
  Fixed-provider/real-process tests verify prompt completion without an independent judge, tracking
  allowing an old process to finish, dynamic/replan cancelling it, and no-stagnation suppressing the
  diagnostic actor. Replan waits for settlement before clearing the main context and invalidates all
  dependency epochs; source files remain intact.
- Added a durable event observer and transactionally keyed control delivery. The observer test proves
  a scripted correction is saved before the next model dispatch; restart tests reject duplicate or
  changed-content deliveries. This supports precise frozen triggers without a client/model timing race.
- Terminal incomplete/invalid tool responses now retain authoritative provider usage while remaining
  non-executable. Actual local HTTP/SSE test verifies the shared money meter settles that usage rather
  than charging the full reservation as unknown. Runtime also saves the failed payload and usage and
  gives the next model round actionable feedback. Interrupted streams lacking usage remain conservative.
- Full `cargo test`: **98/98** integration tests passed after mechanisms and accounting changes.
  Current monetary ledger: **0.39785736 RMB**, no new paid calls in this checkpoint. The experiment
  executor/reporting are still unimplemented; concrete next interfaces and recovery rules are in
  docs/eval-execution.md. The corpus freeze is not evidence that the study has run.

- Evaluation corpus frozen before model trials: `eval/frozen.json`, created
  `2026-10-21T16:37:41.009915+00:00` (2026-10-22 local), digest
  `c19d6137492f5401bba7a8223d5045d94da620e4755537c32034f27c2851fd4c`.
  Contains 12 development and 24 held-out tasks, balanced across Python/Rust/TypeScript and new/existing
  projects, six functional checks each plus about/protected files, scripted corrections and a frozen
  external oracle. Rubric, thresholds, interleaving seed, limits and uncertainty method are hashed.
- All 18 existing source fixtures were compiled/run in independent temporary projects. Each retained
  its about command and caller-owned files, and each failed at least one initial acceptance check.
  Record: artifacts/corpus-preflight/summary.json. Both public-check phases compile for all 36 tasks.
  Strict JSON comparison rejects boolean/number confusion. Freeze/tamper tests passed (2/2).
- Ruling: this is a custom controlled corpus with disclosed three-second simulated CI latency,
  not an estimate of general software-engineering performance. Automatic nudges are reported as
  scripted interventions, not observed human labor. Human rubric cells remain empty until reviewed
  by a person. Removing selective repair is already the replan baseline; reuse those paired traces
  for that ablation instead of counting duplicates as independent evidence. Main conditions plus
  the separate no-stagnation ablation yield 540 scheduled trials, still constrained by the single
  50 RMB account. No paid trial on this corpus has started.

- Terminal checkpoint: Ratatui/Crossterm now provide continuous input, Chinese/grapheme editing,
  bracketed multiline paste, scrollback, status/diff/budget and lifecycle controls. A dedicated socket
  reader prevents redraw/input cancellation from discarding partial JSON frames. Replayed events
  cannot double-count budget; late snapshots cannot rewind a newer status. The latter and exact-width
  newline layout bugs were reproduced and corrected RED→GREEN.
- Actual macOS PTY fixture verified paste waits for Enter, Chinese correction delivery, Ctrl-C pause,
  clean detach and an independently surviving server. A real Flash session `ec0a2bf05c9e49a7` also
  received the added -40→-40 requirement through the TUI and completed after the client detached.
  External `python3 -B -m unittest -v test_temperature.py`: 3/3 passed. Original 0→32 and 100→212 tests
  remain. Session was stopped after verification. Replay bytes: artifacts/tui-live.ansi; report:
  artifacts/campaign/terminal-probe.json. This is an engineering smoke, not a comparative quality run.
- Cumulative accounted experimental spend: **0.39785736 RMB / 50 RMB**, zero uncertain or outstanding
  reservation. The probe's task token ceiling was raised through `/budget` to 500000, which does not
  alter the shared money cap. Overall usage in this probe: 230483 tokens.
- Search/fetch attempt quotas now persist before network dispatch and survive daemon restart.
  UI polling uses a small status projection rather than retransmitting actor/image history; a real
  socket test verifies a 420 KiB model history is excluded from the status response.
- Latest full `cargo test`: **89/89** integration tests passed. Native Terminal window visual review
  is still unverified: Computer Use explicitly prohibited access to com.apple.Terminal. No alternate
  UI automation was used to bypass that restriction. User-facing usage: docs/terminal.md.

- Model/tool fixtures live in tests only. Real validation reads the ignored `.env` key into subprocess
  environment; ordinary shell sessions still do not automatically inherit it.
- Keep the existing 50 RMB account at artifacts/campaign/money.sqlite across every real run.
- Current application-layer command policy is not an adversarial shell sandbox.
- Remaining: browser authorization/recovery refinements, native terminal visual review,
  execution/reporting of the frozen 36-task comparisons/ablations, macOS packaging and fresh final review.
- Also verify paused side questions, clarification cancellation and stale evidence; add explicit
  assumption/acceptance-note storage without letting model notes replace user requirements.
- New Session snapshots have no compatibility requirement yet; keep one clean schema as implementation
  evolves. Do not add legacy readers just for these unreleased development snapshots.

## 2026-10-22 evaluation and execution checkpoint

- Implemented the real daemon-based evaluator: deterministic 540-entry schedule, opaque workspaces,
  pinned source/executable hashes, durable once-only correction delivery, external source snapshots,
  bounded standardized rescue, shared per-session/global money attribution and task-paired bootstrap
  reports. Recovery retains the original session and refuses to recreate previously billed work.
- First five development pilot trials all exhausted before a runnable CLI. Their source, events,
  original corpus and freeze remain under artifacts/evaluation/pilot. An explicit pre-holdout
  amendment raises the equal task ceilings to 500,000 tokens / 8,192 output, and discloses offline
  serde_json availability. Functional inputs/outputs, thresholds and the 50 RMB total cap are unchanged.
  Current registered digest: 2e710f9e3af45a70edec4654b3fb9827e19b770e455894a4c2e652ae2267dbbf.
- The next five trials (dev-intervals, repetition 1, all conditions) passed all external checks, but
  each exhausted before the agent's own completion gate. This is not a reliability improvement claim.
  Their pinned executable remains in artifacts/evaluation/development; subsequent development trials
  are running with that same pin. No holdout trial has run. Accounted spend after those five was
  3.58795320 RMB, with zero uncertain/reserved; consult the live ledger for later spend.
- Queued stale checks and stale diagnostic loops were reproduced and fixed. Active time now uses a
  monotonic clock sampled independently of event volume; continuous-stream starvation was reproduced
  RED then passed. Resuming stopped completed work triggers a fresh review instead of becoming idle.
- Model assumptions and operational acceptance notes have separate types, model authorship and exact
  user-source quotes for acceptance. They cannot mutate requirements or authorize actions. Requirement
  revisions expire dependent notes. Explicitly observed build artifacts are rehashed for acceptance,
  including deletion and changes inside normally excluded build directories.
- Browser input now checks the live document origin immediately before interaction. Approval binds
  origin and arguments; readonly actors cannot obtain remote mutation rights from an approval. Actual
  Chrome script-navigation regression passes. Its first fixture incorrectly assumed macOS could reach
  127.0.0.2; a socket probe disproved that and the fixture now uses a reachable IPv6-mapped loopback.
  Browser evidence also requires unchanged source hashes across inspection.
- MCP uses the official SDK's Auto lifecycle. The earlier discover-only ruling was unnecessarily
  restrictive; SDK negotiation supports initialize servers without a second client implementation.
  A real child server reproduced the failure and passes after this change. Approval IDs returned from
  live tool checks are now persisted and used to requeue the exact pending action, with a second
  authorization required if its origin changes before retry.
- Full cargo test --all-targets passed after these engineering changes. New checks include actual
  Chrome navigation, both MCP lifecycle modes, origin-sensitive authorization retries, time limits,
  note provenance and build-artifact freshness. CLI documentation is now in README.md.
- Still open: the full budget-constrained study, report threshold diagnostics and human review,
  finer file-dependency scope, further paused/clarification lifecycle review, macOS release packaging,
  and the required fresh final review. Native Apple Terminal visual review remains unverified.

## 2026-10-22 protocol checkpoint

- Twenty development trials now finished: 16 passed external acceptance; 4 encountered a Responses
  history-format defect. The fifteen interval trials are all externally passing; only one reported
  its own completion before exhaustion. The five decoder trials exposed the interruption ordering
  bug. Full incident, request IDs, actual replay and study limitations are in
  docs/incidents/2026-10-22-tool-history.md. The old study executable and results remain unchanged.
- Wire serialization now delays an interleaved user message until all outputs in its tool batch;
  the persisted chronology is untouched. A real replay of the original rejected request succeeded,
  using 20,790 input / 122 output tokens, without executing returned tools.
- Current shared ledger after replay: 10.36972104 RMB accounted + 25.952256 RMB uncertain, zero
  reserved. The twelve HTTP 400 replies supplied no usage; their conservative allowance has not
  been released. The user was asked for billing evidence, not additional spending authorization.
  No further paid study batch has started after the defect was observed.
- Time now checkpoints during a pending request, so a crash cannot lose an entire long stream's
  elapsed duration. These heartbeats do not reset the evaluator's meaningful-activity stall timer.
  Paused side questions preserve the paused state. Reporting reads the live, readonly ledger,
  rejects duplicate/missing repetitions, computes strongest-baseline threshold diagnostics, and
  cannot establish the research gate from partial/development results or timing proxies.
- Current checks: cargo clippy --all-targets -- -D warnings passed; cargo test --all-targets passed
  **119/119**; cargo build --release --locked passed. Release doctor reports source hash
  b3efcf59a650ea4f94efad1bc6b0f36ae5c9f92b352cd65d9bf417180ad06728. Packaging and final review are next.

- Staged macOS ARM64 package: dist/engineering-checkpoint/loh-macos-arm64-0.1.0.tar.gz,
  with source/binary hashes, docs and SHA-256 sidecar; no credentials, campaign state or project
  artifacts are included. Mach-O architecture and ad-hoc signature integrity verified. Release-mode
  daemon and actual PTY tests passed 8/8. This is a checkpoint, pending fresh review fixes.
- Real official Search and Fetch calls succeeded through WebClient and the shared account: two
  search results, 10,687 bytes of the requested Responses documentation. Artifacts: artifacts/web-live.
  Configured-rate cost was 0.01000000 RMB. Ledger now 10.37972104 RMB accounted +25.952256 RMB uncertain,
  no reservations. examples/web_probe.rs and the release doctor provide reproducible integration checks.
- The one fresh whole-branch reviewer is active against 7759471. Preliminary reproduced issues include
  stopped-session CLI resume, TERM-resistant descendants after leader exit, and uncertain external
  effects through stdio MCP. Final classification and the required regression/fix pass are pending;
  do not label the staged package production-ready or install it as the final delivery yet.

## Final engineering review fix pass

- Fresh reviewer completed the whole-branch review at 7759471: four Important findings, no Critical
  finding, no deferred minors. Findings and all declined-to-judge rulings are recorded in
  docs/final-review.md. Review did not establish research effectiveness.
- Final: fixed stopped-session CLI resume — durable intent before launch, acknowledgement after
  consumption; cli_resume_persists_intent_before_starting_a_stopped_daemon RED→GREEN.
- Final: fixed evaluator recovery after daemon stop — finalize without starting new model work;
  real_daemon_trial_grades_rescues_and_resumes_without_duplicate_model_calls RED→GREEN.
- Final: fixed uncertain external/unknown stdio MCP effects — preserve concrete effect provenance
  and pending receipts across cancellation/restart/config changes; real localhost mutation regression
  RED→GREEN, a retry remains blocked until reconciliation.
- Final: fixed process-group settlement after leader exit — track validated surviving members,
  retain an inherited ownership marker for orphan recovery, escalate and confirm exit before saving
  tool completion. Stubborn descendant regression RED→GREEN; normal completion, persisted ownership
  and mismatched kernel process identities also pass.
- Final: fixed opaque approval display — exact tool plus redacted parameters and full private preview
  are reviewable before /approve; original arguments still determine the approval ID. Rendered
  authorization regression RED→GREEN; common credential forms remain hidden in its preview.
- Strict Clippy and the full development suite passed **126/126**. Release-mode daemon, terminal,
  process-group and MCP tests passed **20/20**, followed by a clean default release build. No
  re-review was dispatched: the skill calls for one author fix pass with covering regressions.
