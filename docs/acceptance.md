# Engineering acceptance

Product: **Logit- Harness**. Command: **`loh`**. macOS ARM64, version 0.1.0.
The installed executable is `/opt/homebrew/bin/loh`. Configure credentials with `/setup`, `loh setup`
or `DEEPSEEK_API_KEY`. Build and use instructions are in [README](../README.md).

## Current CLI polish — 2026-10-23

The editor, provider error handling, address checks and daemon cleanup were reviewed against the
user's bug report. The [qualified findings and limitations](bug-review-resolution.md) remain part
of this acceptance, including intentional loopback development access and unchanged historical
uncertain charges.

- Product commit: `9bc0cba`; source hash
  `cbf62e3f64b1a74f60d3fed4265c9d792a6e9329e04ba746fd932d646b67c5a2`.
- Full locked all-target regression: **266 passed, 0 failed**, 53 result groups; formatting,
  strict Clippy and release build passed. Independent review has no remaining confirmed P1/P2.
- Installed macOS ARM64 executable has a verified ad-hoc signature and SHA-256
  `f26534930f6a379bb7e9ac78d6848519ac15efb3cf04e3d5858db3994c7dfd30`.
- Raw engineering, review and installation evidence: `artifacts/polish-2026-10-23`.
  The original process-fixture readiness failure and the later atomic-publication correction
  are both retained; the failed run is not counted as a passing full suite.
- Installed editor, layout and integrated-interaction PTY checks passed with matching source
  hashes. They cover the 34-cell wide-character boundary, Unicode line/word edits, menus, native
  scrollback, resize, background continuation, cancellation and 401-event history. The editor
  made no model requests; the other two fixtures used 18 localhost requests and no paid calls.
  Captured character grids were visually inspected; native-window appearance remains unclaimed.
- This round makes no new paid-model quality claim and does not reopen the exhausted experimental
  allowance. The byte-unit tool clarification has engineering coverage but no paid retest.

## Previous interaction build — 2026-10-22

The conversation CLI was rebuilt with a single startup blue L, bottom-docked input, mode-specific
prompts, measured activity and compact tools with explicit source-truncation notices. Completed
records remain in native scrollback. Repeated menu/editor transitions preserve the complete row
sequence; full tool streams and final metadata remain searchable, expandable and exportable.

- Product commit: `db0253b`; source hash:
  `2a1ed33994e889325e08eda817d1f45a5405abeb05115d5f51591629b5e4faf0`.
- Full locked all-target regression: **237 passed, 0 failed**, 51 result groups. Formatting,
  strict all-target Clippy and the locked release build passed.
- macOS ARM64 release installed atomically with verified local ad-hoc signature. Executable SHA-256:
  `d864e5cab85411e13b315012849b074602094196cdeeb488b32f94dfa8b3510b`.
- Review corrections also cover isolated complete `/diff`, preserved persisted tool streams,
  read-only resume with elapsed-time enforcement, and valid contained instruction-directory paths.
- Source-matched delivery logs and receipts: `artifacts/interaction-delivery/final`.
  Earlier checkpoint evidence is retained separately and does not establish this build's results.
- Installed-binary macOS PTY layout, interaction and Enter checks passed: 11 + 7 + 1 localhost
  fixture requests, zero paid requests. Captures cover three menu cycles, three nested editor/menu
  cycles, subsequent replies, narrow/tall resize, one startup brand, background work across `/new`,
  cancellation, masked setup and 401-event searchable history. Source and executable hashes match
  the installation receipt. Colored grids were visually inspected; native-window acceptance is
  still not claimed.
- Real Flash greeting, thanks and knowledge samples each completed in one request with no tools,
  goals, workers, verifier or project guidance. Their aggregate reported input cache hit was
  4,736 / 6,184 tokens (76.58%). This is a small functional sample, not a controlled cache gain.
- The real project-read sample is **not accepted**: the model treated an unspecified `limit` as
  line count, first reading 5 and 1 bytes, then obtaining the complete file with 300. The probe's
  request-count guard cancelled the fifth request before its answer. Tool byte units need clearer
  model-facing documentation and a subsequent paid retest; the CLI fixtures do not prove that
  model behavior. Read-only boundaries held and the workspace remained unchanged.
- This probe added 0.0087644 RMB of settled configured-rate charges and 2.105344 RMB of uncertainty
  for the cancelled request. The unchanged 50 RMB account now has used 19.77662472, uncertain
  30.220288, reserved 0 and conservative available 0.00308728 RMB. No uncertainty was cleared or
  budget reset; another full reservation does not fit. See `flash.json` and `flash-analysis.json`.

The frozen research executable and its real correction example below are unchanged. They are not
replaced by the interaction build or counted as evidence of a new reliability improvement.

## Frozen research executable

The following measurements belong to the frozen research build. Later cache, terminal and API
setup changes have separate engineering evidence in [progress](progress.md); they do not inherit
this build's real-provider acceptance results.

- Source hash: `1c16dc63d232a4e91ed4cefb06dd68dbc61b237d817c4c5ace32f9e068cd3ba7`.
- Executable SHA-256: `41242b0b92eae3a9c3103099e088cebfd86cb27922781020f60134ad4827533d`.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo test --all-targets`: 130 passed.
- Release-mode terminal, daemon, process-group, MCP, domain, steering and verification tests:
  36 passed. A default `cargo build --release --locked` followed these checks.
- Installed binary architecture, local ad-hoc signature, version and doctor output verified.
  Logs: `artifacts/correction-scope`. No notarization or public publication is claimed.

## Real correction example

Session `e2cb3548e7ef441d` used the installed executable and the official DeepSeek Flash endpoint.
It started detached, completed an existing Python todo CLI's JSON storage, and received a caller
correction while its verification process was running: use SQLite while retaining the CLI and data.
The caller also changed its verification script and appended a note during execution.

The independently executed checks confirm:

- JSON persistence worked before the correction; the initial public script did not expose SQLite.
- The correction committed requirement revision 2 and retained the core requirement as active.
- The old verification process was cancelled and settled before replacement work proceeded.
- SQLite imported legacy IDs/data once, preserved the original JSON bytes, and persisted changes
  across fresh CLI processes. The database header and integrity check were valid.
- CLI and caller-note hashes were preserved, including the concurrently appended note.
- The agent reached independent Completed status before the test driver stopped the daemon.

Recorded input-to-revision latency: 3.76391 seconds; input-to-old-process-exit: 3.88944 seconds.
The run used 261,397 tokens and 134 active seconds; configured-rate cost was 0.33823928 RMB.
No rescue message was sent. These are observations from one example, not an improvement estimate.
Full source, public checks, external acceptance, events and final state: `artifacts/storage-acceptance`.

Two preceding probes remain visible: `artifacts/release-smoke` leaked a future constraint through
its public test, and `artifacts/storage-correction` exposed an invalid narrowed completion scope.
Neither is counted as full correction acceptance. They led to recorded fixture/engineering repairs
before the held-out executable was frozen.

## Other integration evidence and limits

Actual Chrome tests cover navigation, input, screenshots, DOM/console access and origin-sensitive
authorization. The MCP tests use real subprocess and HTTP transports, including uncertain external
effects and crash recovery. PTY tests cover Chinese input, paste, pause, detach and resume. Real
Responses history replay and official Search/Fetch calls have separate persisted receipts.

Apple Terminal's native window visual inspection was denied by Computer Use and remains unverified.
Ordinary shell policy does not claim adversarial arbitrary-code isolation. Other operating systems
have not received equivalent acceptance.

The research gate is independent of these engineering checks. The original development stage is
incomplete, and the budget-limited held-out stage has its own executable pin and report. Missing
repetitions and human review cannot be replaced by this successful example. See
[allocation](study-allocation.md) and [evaluation execution](eval-execution.md).
