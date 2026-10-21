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
