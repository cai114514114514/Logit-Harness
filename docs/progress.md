# Weft implementation ledger

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

## Implementation watch list

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

- Model/tool fixtures live in tests only. Actual API key still absent from execution environment.
- Keep the 50 RMB account durable and shared; do NOT run paid inference before wiring reservations.
- Current application-layer command policy is not an adversarial shell sandbox.
- Scope currently contains requirement versions and observed file hashes. Add explicit step leases
  for narrowly scoped workers; unknown dependencies must use all active requirements.
- Initial goals currently have only r1. Add provenance-checked decomposition of explicit source quotes
  before evaluating selective repair, so edits to a storage requirement need not invalidate a shortcut.
- Current ask_user blocks the main actor; make questions requirement-scoped and allow independent work.
- New Session snapshots have no compatibility requirement yet; keep one clean schema as implementation
  evolves. Do not add legacy readers just for these unreleased development snapshots.
