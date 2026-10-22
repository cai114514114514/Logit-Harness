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

## Installed engineering release

- Installed `/opt/homebrew/bin/loh`; `loh --version` returns `loh 0.1.0`. The installed executable,
  `target/release/loh` and the packaged executable are byte-identical. `doctor --json`, Mach-O ARM64
  inspection and local ad-hoc signature verification pass. No notarization or public distribution
  has been performed.
- Package: `dist/release/loh-macos-arm64-0.1.0.tar.gz`, with its SHA-256 sidecar and manifest.
  Source hash: `3e50639ce50ee15d3e5c4cd0a32506b3236d112b549ead8970b4d59b1849f933`.
  Binary SHA-256: `197dfdfd8773519e7330405263dc1cf8951c8e9e0bcf77e5a0643347cedb8b36`.
  Exact build/test logs and installation receipt are in `artifacts/release`.
- The ordinary shell still needs `DEEPSEEK_API_KEY` exported, as documented in README.md. Test
  runners receive the existing local credential through their environment without printing it.
- Engineering delivery does not finish the research study: the original 20 development trials,
  the preserved protocol defect and conservative uncertain charges remain reportable. No held-out
  trajectory or human rubric review has been completed at this checkpoint.

## Installed-binary smoke and verification feedback repair

- Real session `4d903ae72fd34848` reached its own Completed state in 212 active seconds, using
  904,338 tokens and 0.73989896 RMB of configured-rate usage. The caller's correction cancelled the
  live verification process; the final external SQLite/data-preservation checks and protected-file
  hashes passed. Full replay and acceptance: `artifacts/release-smoke`.
- This smoke is **not** evidence of JSON-to-SQLite adaptation: its initial public oracle accidentally
  included the later SQLite assertions, and the model implemented SQLite before the correction.
  That contaminated fixture and its failure of initial task fidelity are retained and disclosed.
  A separate `artifacts/storage-correction` fixture exposes only the current JSON contract before
  correction and saves an external initial-storage check before introducing SQLite.
- The run also exposed unnecessary implementation work: a verifier reported success while citing
  a failed optional concurrency check; the harness sent this contradictory report back to the
  implementer. Passing verdicts with failed/obsolete cited evidence now return an actionable error
  to the same verifier to refresh evidence or correct its verdict. A genuinely failed requirement
  still returns to implementation, and invalid evidence still cannot establish completion.
- The actual command-based regression failed before repair and passes afterward. Strict Clippy
  and the complete **127-test** suite pass. No held-out trajectory has started, so this change does
  not tune against held-out outcomes. Release checks and the corrected fixture follow before pinning.

## Correction scope repair before holdout

- The corrected fixture first confirmed real JSON persistence, then delivered the SQLite correction
  while the old verification process ran. Session `f9d030db55e44aed` reached Completed after 120
  active seconds / 378,801 tokens; all external SQLite and caller-file checks passed.
- Inspecting the saved requirement graph exposed an invalid success scope: the classifier returned
  `replace` together with updates to r1 and two additions. Replacement deactivated r1; its updated
  core requirement was silently excluded from final acceptance. This trajectory is retained in
  `artifacts/storage-correction`, explicitly **not** accepted as full requirement-aware completion.
- Replacement and updates are now mutually exclusive and rejected before any revision/history
  mutation. The same classifier receives a concrete error and can retry as refine. A valid whole-task
  replacement retains the user's complete verbatim objective as an active acceptance requirement,
  even when model-derived subrequirements omit a clause. The classifier instructions distinguish
  changing a backend from replacing the entire task.
- Two domain regressions failed before the repair, then passed. A runner integration confirms the
  malformed classification is retried, only one revision is committed, and the original core
  requirement remains active. A final `artifacts/storage-acceptance` run will check this invariant
  alongside real initial/final storage, old-process cancellation and preservation of caller edits.
- No held-out trajectory has started. Old binaries and trajectories remain distinct; none of these
  fixes or examples can be represented as a held-out research improvement.

## Final storage acceptance and held-out freeze

- Session `e2cb3548e7ef441d` passed the complete engineering example on the installed release:
  real initial JSON persistence, mid-command correction, cancelled old test, retained active core
  requirement, SQLite migration, repeated-process persistence and unchanged caller files. It reached
  independent Completed status before the driver stopped the daemon. No rescue prompt was sent.
- User-input to applied revision: 3.76391 seconds; to confirmed cancellation: 3.88944 seconds.
  Total: 134 active seconds, 261,397 tokens, 0.33823928 RMB configured-rate cost. These are single-run
  observations, not a controlled speed or reliability comparison. Replay: `artifacts/storage-acceptance`.
- Current release source: `1c16dc63d232a4e91ed4cefb06dd68dbc61b237d817c4c5ace32f9e068cd3ba7`;
  binary SHA-256: `41242b0b92eae3a9c3103099e088cebfd86cb27922781020f60134ad4827533d`.
  Strict Clippy, **130/130** full tests, **36/36** release-mode tests, default release build and
  installed signature/hash verification pass. Logs are in `artifacts/correction-scope`.
- The pre-holdout allocation and exact pin are in `docs/study-allocation.md`. Development remains
  incomplete at 20/180; the remaining admissible account balance is allocated to the fixed held-out
  schedule. No further tuning based on held-out outcomes is permitted; missing research and human
  reviews will remain explicit.

## Budget-limited research checkpoint

- Held-out driver finished with `budget_limited`: **15/360** trajectories, one distinct task,
  **13/15** external passes, **0/15** independent completions. Thirteen runs exhausted task token
  admission; the last two exhausted shared money admission. No core, prompt, tool, task or oracle
  change occurred after the held-out pin. Native processes are settled and stopped.
- Final shared account: 19.76413632 RMB accounted + 28.114944 RMB uncertain, zero reserved;
  liabilities 47.87908032 RMB. Remaining 2.12091968 RMB cannot cover the fixed 2.162688 RMB next-call
  reserve. The new uncertainty is one network send failure; the original twelve HTTP 400 allowances
  remain unchanged. No funding, refund assumption or replacement account was introduced.
- Full research is incomplete: 505/540 scheduled trajectories remain, and human review is pending.
  The one-task paired comparison has no task-level confidence interval and shows no scripted
  intervention reduction versus the strongest observed baseline. Effectiveness is not established.
- `docs/research-results.md` presents the counts, caveats and budget; original per-trial reports,
  source snapshots, events, frozen programs and ledger remain available for inspection/recovery.

## Packaged delivery

- Current package: `dist/delivery/loh-macos-arm64-0.1.0.tar.gz`; archive SHA-256:
  `46b5cfd0a652577e3f758d3b6bf702cb7ff9f74131bde4efcf639922ec44338c`.
  Older `dist/release` and `dist/engineering-checkpoint` packages remain historical and superseded.
- Extracted the current archive into a fresh temporary directory. Its binary matches both the
  installed `/opt/homebrew/bin/loh` and the frozen held-out binary; doctor, local signature and
  offline replay of the Completed→Stopped event tail pass. The package includes the offline reader
  and allowlisted synthetic event/acceptance records, with no credentials or campaign database.
- Verification receipt: `dist/delivery/verification.json`. No paid operation was used for package
  validation. No evaluation daemons or model reservations remain. Research remains incomplete for
  the documented budget and review reasons; the overall approved research objective is not marked
  complete.

## Full-scope audit continuation

- The preceding goal turn was substantive progress. The present audit reconfirmed the terminal
  driver/process state and unchanged financial blocker; no provider request or ledger adjustment
  was made. Full requirement-to-evidence mapping is in `docs/completion-audit.md`; engineering
  checkboxes now reflect the verified work, while complete research and human review stay open.
- Exported `artifacts/campaign/billing-review.json` with 13 uncertain charge records, 12 HTTP 400
  provider request IDs and one network-send failure. `scripts/billing.py` opens both stores readonly,
  omits credentials and bodies, and does not assume exact per-request charge attribution. The ledger
  hash stayed unchanged during export; it is evidence for future billing review, not a refund.
- The audit found missing dedicated HTTP 429 coverage. Added four actual localhost HTTP regressions
  for retry integrity/notifications, attempt limit, backoff cancellation and malformed Retry-After.
  `cargo test --locked --test rate_limit`: 4/4 passed; targeted strict Clippy passed. No execution-core,
  dependency, frozen task, prompt, tool or scoring changes were made. Logs: `artifacts/completion-audit`.
- The installed/research/package executable pin remains unchanged. No additional paid experiment is
  admissible under the current account. Overall completion remains unproved; do not replace the
  505 missing trajectories or actual human review with this offline audit.

## KV cache follow-up after study freeze

- The user's cache requirement prompted a separate engineering change. The baseline contains 944
  reported calls with a token-weighted input hit rate of 50.29% (holdout 44.47%). In 789 adjacent main
  requests, 474 changed the first system instructions. `scripts/cache_report.py` reproduces the
  read-only usage and prefix audit; totals match an independent SQL aggregation.
- Stable role instructions now precede append-only runtime state updates. Unchanged state is not
  resent; changed sections explicitly replace old sections, preserving empty values/deletions.
  Compaction re-seeds full current state. Tool-batch pairing, full reasoning history, provenance,
  cancellation and file guards remain enforced. `/budget` and the terminal expose provider cache
  usage, with replay deduplication and missing-usage labels.
- A failing real-runner prefix regression was captured before the fix. The complete current
  `cargo test --all-targets` suite passes 138 tests; strict all-target Clippy and the locked release
  build pass. The recovery/worker fixtures now read task state from the chronological input rather
  than assuming it appears in `instructions`. Logs and baseline: `artifacts/cache/`.
- Installed `/opt/homebrew/bin/loh` now has source hash
  `9c60ee2a39aa7bf3596d75417efb81f210411525331a73c34fcd931f9fccd7d2`.
  Its doctor and ad-hoc signature verify. The previous frozen research executable and
  `dist/delivery` remain unchanged. The cache build is packaged separately under `dist/cache`;
  any bundled SQLite correction replay predates this change and is historical evidence only.
- No paid request was made. A real-provider cache/latency/quality comparison remains pending the
  existing budget blocker. Request-prefix equality does not prove a particular hit rate or speedup.
  Full design, measurements and limitations: `docs/cache.md`. The overall research goal stays open.

## Terminal redesign after local CLI comparison

- Inspected the actual local Claude Code 2.1.304 (not logged in) and Codex CLI 0.159.3 startup,
  composer and slash-command menus without submitting a coding task. Closed those inspection
  clients afterwards. Findings and design choices: `docs/terminal-review.md`.
- Replaced the raw startup prompt with a shared multiline editor, compact welcome screen,
  filtered command palette, completion, history and recent-session picker. The UI uses the normal
  terminal buffer. Tool commands, paths and results stay grouped, failures remain visible, Markdown
  is readable, and Ctrl-O expands output. The existing KV statistic stays in the footer.
- Added read-only saved-conversation viewing when the socket is unavailable. Viewing does not
  restart execution; resume is explicit and requires credentials. Fixed a real PTY transition
  failure caused by synchronous cursor queries racing the previously created async input reader.
- Complete suite: 143 passing tests; strict all-target Clippy and locked macOS release build pass.
  Actual installed-binary PTY checks pass for startup, slash menu, completion, multiline paste,
  resize, session selection, archive view, tool failure, output expansion and keyless resume refusal.
  Evidence: `artifacts/terminal/interaction.json`, logs and rendered PTY grids. These are not native
  Apple Terminal-window screenshots; the example conversation is clearly marked synthetic.
- Installed source hash:
  `52c6a53a55ba1cb202188d2e5cc1e4b8755292d02b18a3580a70ee7b614c10b3`.
  Current client package: `dist/terminal/loh-macos-arm64-0.1.0.tar.gz`. Earlier delivery/cache packages
  and the frozen research executable are retained. No paid provider call or model/harness strategy
  change was made. This does not resolve the independent research-budget blocker.

## Blue L identity

- Designed the requested L icon with the built-in imagegen tool: a white geometric L, diagonal
  right terminal and electric-blue rounded tile. Project asset: `assets/brand/loh-icon.png`
  (1254 × 1254 RGBA). Design notes and generation prompts: `assets/brand/README.md`.
- Replaced the terminal's generic diamond with a white L on a blue badge. Identity, selection and
  progress now use blue; success and failure retain their semantic colors. The actual terminal uses
  a text badge rather than attempting to render a PNG. README and packaged releases include the art.
- The 14 existing CLI/terminal/layout checks pass, including actual PTY input and detach. This is a
  small branding change; no model protocol, execution policy, evaluation corpus or billing ledger
  changed. The earlier complete 143-test terminal validation remains separately recorded above.

## Terminal-native L mark

- Replaced the single-cell L badge with a three-row blue block-character L, placed beside the
  product name, model and project directory like the requested Claude Code startup composition.
  The mark retains its silhouette without color, and its half-height right tip echoes the app icon.
- Extracted the header into `src/cli/terminal/brand.rs`. Existing 14 CLI/terminal/layout tests pass;
  actual PTY checks and visual review at 100 and 54 columns pass. Logs: `artifacts/terminal/mark-*`.
  Locked macOS release build and executable signature verification pass; installed `loh` updated.
  No execution, model, cache or evaluation behavior changed.

## API setup

- Added `/setup` to welcome, live conversations and saved-record view, plus standalone `loh setup`.
  The blue form has a Base URL and a separate masked key editor. Enter advances/saves, Tab switches
  fields, Ctrl-U clears and Escape cancels. Saving does not initiate a provider call. Tiny terminals
  request more space; changing the endpoint refreshes the existing-key indicator.
- Credentials are endpoint-bound and saved atomically in `credentials.json` with mode 600 inside
  the private state directory. This is permission-protected plaintext, not Keychain or encryption.
  The secret never enters chat input, history or session events. Ordinary config preserves unrelated
  settings; nonempty `DEEPSEEK_API_KEY` retains priority. New daemons load saved credentials without
  an environment key; existing providers keep their initialized credentials until restarted.
- Added regressions for endpoint binding, permission/symlink rejection, config preservation, masked
  doctor output, environment priority and redaction when HTTP errors echo credentials. The detached
  CLI/daemon test now reads a saved fixture credential and completes its real tool workflow.
- Complete suite: 148 tests passed. After a small viewport guard adjustment, 21 affected tests passed
  again; strict all-target Clippy and the locked release build pass. Actual installed-binary PTY
  checks cover hidden paste, save feedback, cancellation, endpoint changes, resize, restart lookup
  and absence of chat/session creation. Evidence: `artifacts/setup/` logs, `interaction.json`,
  `install.json` and rendered PTY grids; these are not native Terminal-window screenshots.
- Installed `/opt/homebrew/bin/loh` source hash:
  `1dc1138cdffafa46aa9917ce0a1754b6617a403705d6fc3f2c41297ac402b698`.
  macOS ARM64 architecture, ad-hoc signature and isolated-home doctor verified. Packaged separately
  under `dist/setup`. Earlier packages and the frozen research executable remain intact.
- No real key was entered or copied into the user's settings by these tests, and no paid request
  was made. This validates configuration and transport handling, not the user's live authentication
  or an improvement in real-model reliability. The research-budget blocker remains unchanged.

## Lightweight interaction delivery — runtime checkpoint

- Implementing the approved [interaction design](superpowers/specs/2026-10-22-interaction-design.md).
  Runtime commits `0a6ae73` and `1e1ea5a` separate ordinary conversations from engineering goals,
  defer project discovery, preserve plan mode, archive independent goals, queue unrelated tasks,
  and correlate actual user turns, responses and actions. Engineering completion still requires
  independent verification.
- The user-reported Enter failure was reproduced on the installed old binary: a completed daemon
  retained the workspace lock and prevented new startup. Execution-scoped ownership fixes that
  lifecycle error. Read the [incident and verification record](incidents/2026-10-22-enter.md).
- Initial full regression: 168 passed. Review corrections: 53 covering tests passed; strict Clippy,
  formatting, locked release build and signature checks pass. Independent scoped review has no
  open findings. Actual installed-binary PTY Enter check passes with an isolated local HTTP fixture.
- Interim runtime installed at `/opt/homebrew/bin/loh`; source hash
  `caa2d3919041ade4808e73bbe093638a682bfcd25a186f0ae8150efc20a6f0cf`.
  Unified command services and native scrollback/history integration are still in progress.
- Real installed-runtime Flash greeting `044ab632e08542a0`: one response, zero tools and goals,
  Conversation/Waiting. Input 1822, output 10, cached 0, configured-rate charge ¥0.003724.
  Shared 50 RMB ledger: used 19.76786032, uncertain 28.114944, reserved 0, conservative available
  2.11719568. No historical uncertainty or cap was changed. This is one greeting sample; broader
  model routing and reliability remain separate from fixture engineering tests.

## Command backend checkpoint — public development snapshot

- Commit `b80da9f` adds the shared command directory/parser/service, source-aware Markdown workflows,
  configuration snapshots, and MCP controls. Its complete regression has 190 passing tests; strict
  Clippy and formatting pass. These results do not establish terminal integration or close review.
- Independent review found two open issues: `/tools` currently selects tool-call history rather
  than exposing the requested capability catalog; the new read-only Git inspection path can invoke
  repository-configured helpers. Both require correction before this backend is accepted. Scalar
  argument whitespace and several help descriptions also need small corrections.
- User feedback adds an explicit terminal acceptance requirement: show actual current activities,
  measured elapsed/wait time and blocking owner, instead of retaining the last model-request label.
  The runtime waiting-status correction is covered by a regression; the activity panel and complete
  native history remain Task 3.
- The GitHub upload is a source-history checkpoint, not a release or a claim that the approved
  interaction plan is complete. Local credentials, session databases, provider logs, build outputs
  and packaged experiment artifacts are excluded from tracked uploads.

## Command backend review corrections

- Commit `e625307` resolves both Important and both adjacent Minor findings from the preceding
  checkpoint; scoped re-review found no new Critical or Important issues. `/tools` now inspects
  actual registered capabilities without requiring a session or provider; past calls use
  `/history tools [query]`. Scalar argument whitespace and initialization/configuration help align
  with their runtime behavior.
- Read-only Git inspection uses bounded copied metadata with an isolated configuration, cleared
  environment and disabled lazy fetching/helper/submodule execution. Fixtures cover fsmonitor,
  clean/process filters, nested submodules, and linked worktrees with packed refs/worktreeConfig;
  ordinary staged/unstaged patches remain available and the source index stays unchanged.
  Repository-defined conversions and submodule/gitlink changes are explicitly outside this
  inspection result; unsupported object/ref formats fail explicitly.
- Final covering checks: 33 tests passed across commands, command-runtime, extensions, MCP and web;
  strict Clippy, formatting and diff checks pass. The earlier 190-test full run predates this fix;
  the complete final regression remains a delivery gate.
- Terminal integration, visible activity, original-buffer scrollback and full-history UI are now
  in progress under the updated interaction specification. This checkpoint does not claim those
  interfaces are already delivered.

## Interaction integration — reviewed implementation, final delivery checks

- Product commits `6fc65c5`, `a5ca613`, `8262fb2` integrate the shared command service across
  welcome/live/offline views, native scrollback, durable searchable history, visible measured
  activity and recoverable startup failures. `/new` preserves background work; `/resume` retains
  its execution meaning. Local menu actions do not call a model.
- Independent task review closed initial async-submission/session-binding races, stale Skill
  choices, lost source-qualified completion and hidden cancellation settlement. Markdown export
  now preserves complete persisted streams alongside bounded final results, including quiet or
  partially streamed unsettled actions with their true actor and start metadata. Explicit JSON
  export and refusal to overwrite existing files remain available.
- Final engineering checks on `8262fb2`: `cargo fmt --all --check`,
  `cargo test --locked --all-targets` (**214 passed, 0 failed**, 46 result groups), and
  `cargo clippy --locked --all-targets -- -D warnings` all passed. Source-matched logs and the
  summary are under `artifacts/interaction/final-*` and `engineering.json`.
- Debug-build actual macOS PTY evidence covers real fixture commands, cancellation, side questions,
  new chat while old work runs, Chinese input, masked setup, startup recovery, rename/export,
  defaults on a new session, bare Plan without a request, 401-event history, resize and reconnect
  without duplicate settled output. Tracked reproduction: `tests/pty/interaction.py --binary ...
  --output ...`; it uses a local fixture provider with zero paid calls. Captures in
  `artifacts/interaction/debug-final` render captured PTY buffers, not a native Terminal window.
- Terminal/extension documentation is reconciled with the command catalog. Packaging selects
  tracked documentation, brand assets and bundled Skills; unrelated untracked local documents are
  excluded. A temporary-output packaging check passed without modifying user files.
- At this checkpoint, broad final review, final-source release/install, installed-binary PTY,
  bounded real Flash samples and the final package remain delivery gates. The prior real greeting
  and frozen research results above are not presented as verification of this new executable.

## Clear CLI rebuild and interaction delivery

- The user's conversation-layout correction replaces the earlier welcome-style active header.
  The blue L is committed once per client; input and usage sit at the physical bottom after entering
  a conversation, including `/new`, menu/detail return and resize. Conversation, Plan and Engineering
  have distinct prompts. The active area shows actual work, elapsed time, wait reasons and complete
  actionable IDs; compact tools retain failure tails and source-truncation warnings.
- Product commits `21ac1d5`, `3225c98` and `db0253b` implement and correct the rebuild. Independent
  review exposed cumulative blank rows from reverse-index scrolling. Tracking the committed-output
  boundary separately fixed that defect: three menu cycles and three nested editor/menu cycles
  preserve all 38 original settled rows exactly, including headings and blanks. Later replies reuse
  released space. The initial “bounded blank band” interpretation was wrong and is withdrawn.
- Full regression exposed a hidden source-truncation notice; the original failing test was retained,
  structured source-limit notices now survive compact folding, and scoped review accepted the fix.
  Final locked all-target suite: **237 passed, 0 failed** in 51 groups; formatting, strict Clippy and
  locked release build passed. Logs: `artifacts/interaction-delivery/final`.
- Integrated fixes `fd5e937` and `fab5f1d` also close complete isolated `/diff`, full persisted
  stream retention in history/search/copy, read-only resume and elapsed-time enforcement, and
  valid absolute/directory instruction scopes. Escaping paths remain rejected. Task reviews,
  whole-branch audit and scoped corrections have no open load-bearing implementation finding.
- Installed `/opt/homebrew/bin/loh` is macOS ARM64 with a verified ad-hoc signature. Source hash:
  `2a1ed33994e889325e08eda817d1f45a5405abeb05115d5f51591629b5e4faf0`; executable SHA-256:
  `d864e5cab85411e13b315012849b074602094196cdeeb488b32f94dfa8b3510b`.
  Installation was atomic. Existing user daemons and the unrelated local review document were
  preserved; new clients use the new UI, while new conversations use the updated runtime.
- Installed-source layout, integrated interaction and Enter PTY checks all passed, with matching
  source/binary hashes and 19 total localhost requests. The 401-event history, source output,
  cancellation, setup masking, Chinese input and detail return remain functional. Colored actual
  PTY grids were visually inspected; these are not native Apple Terminal-window screenshots.
- Real Flash passed hello, thanks and knowledge samples in one request each, with no tools/goals
  or project initialization. Reported input cache hit across those samples: 76.58%. The project-read
  case remains unaccepted: ambiguous byte-limit wording led to two short reads before a successful
  full read, and the bounded probe cancelled the following request before its answer. This is not
  evidence of a completed project answer or broader reliability gain. Exact observations and the
  next research item are retained in `flash-analysis.json` and [acceptance](acceptance.md).
- The shared 50 RMB ledger remains authoritative: settled used 19.77662472, uncertain 30.220288,
  reserved 0, conservative available 0.00308728 RMB. No additional model reservation fits; no cap
  or historical uncertainty was changed. Frozen research executable SHA-256 remains
  `41242b0b92eae3a9c3103099e088cebfd86cb27922781020f60134ad4827533d`.
