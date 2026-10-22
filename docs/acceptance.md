# Engineering acceptance

Product: **Logit- Harness**. Command: **`loh`**. macOS ARM64, version 0.1.0.
The installed executable is `/opt/homebrew/bin/loh`. Configure credentials with `/setup`, `loh setup`
or `DEEPSEEK_API_KEY`. Build and use instructions are in [README](../README.md).

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
