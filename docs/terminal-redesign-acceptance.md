# Collaboration-first terminal delivery

Delivered 2026-10-24 (design and evidence directory started 2026-10-23).
The approved [B design](superpowers/specs/2026-10-23-terminal-design.md) is implemented.
Run `loh` in a new terminal to use the installed client; existing clients and background
sessions were not restarted. `/sessions` or `loh attach <id>` reconnects to saved work.

## Delivered behavior

- Blue L once at startup, a short welcome, and recent conversations under `/sessions`.
- A stable member strip in the active region, Ctrl-T selection, visible current task and
  control scope, compact overflow, and distinct archived member identities.
- Immediate submission feedback scoped to the original operation/view. Directed pending
  status survives late acknowledgements; pauses and other blockers take precedence.
- Successful steps normally use one line. Same-response read/search batches can be grouped;
  failures, cancellation, timeout and source limits remain visible. Live commands and failed
  commands have bounded output tails. Long commands preserve the actual exit code.
- Full history, search and copy retain raw records. Known routine diagnostics and successful
  delivery receipts live in details. Menus show source/timing once for the selected command.
- Main/member usage is provider-reported and scoped correctly; unknown KV is `—`.

## Exact installed version

| Property | Value |
|---|---|
| Executable | `/opt/homebrew/bin/loh` |
| Version | `0.1.0` |
| Source commit | `44eeeee85e0dd495c8a268c023a81aa1db1166d3` |
| Source hash | `460baba3f9809eede6d52e39c7205404630d5d0ac847ed3835ab687d60fb4ccd` |
| Signed binary SHA-256 | `a007a2e99821128157b24d3124c888f900ed2b91d7bd22d7453951ed473543c5` |
| Installation | Ad-hoc signed, strict signature verification, atomic replacement |
| Previous binary | `artifacts/terminal-redesign-2026-10-23/previous/loh` |

Documentation commits after the source commit do not change the compiled source hash.

## Verification

All evidence paths below are relative to `artifacts/terminal-redesign-2026-10-23/`.

| Layer | Result | Evidence |
|---|---|---|
| Locked full engineering suite | 369 passed, 0 failed, 0 ignored; 61 result groups | `checks/tests.log`, `checks/receipt.json` |
| Formatting, strict Clippy, release build | Passed | `checks/{fmt,clippy,release}.log` |
| Installed editor PTY | Unicode, wrapped arrows, multiline/paste and argument editing passed | `installed/editor/receipt.json` |
| Installed layout PTY | Append-once history, menu cycles, real exit-9 command, detail resize and all four sizes passed | `installed/layout/receipt.json` |
| Installed interaction PTY | Startup failure recovery, 403-entry history, search older than 300, reconnect and no resize replay passed | `installed/interaction/task3-pty.json` |
| Installed team PTY | Three clients, independent drafts, scoped output, local/global controls, reconnect; view operations add no model requests | `installed/team/team-receipt.json` |
| Installation preservation | All 89 process identities present immediately before replacement remained afterward | `install.json` |
| Protected inputs | User bug-review document, frozen research, original campaign and separate observation allowance unchanged | `preservation.json`, `baseline.json` |

Installed receipts match the final source/binary identity. Grids were inspected for the menu,
failure tail, main roster, and 80×24 / 40×12 member views. Layout checks also cover 110×34 and
60×18. The PNGs are captured PTY character grids using the recorder's fonts, not native-window
screenshots. Native scrollback/reflow remains the host terminal's responsibility.

The independent read-only whole-change review required four fixes: retired actors displayed
as executing, ambiguous/missing archived members, Pending hiding pause, and compact Pending
losing its delivery state. All four regressions failed before repair and passed afterward.
The original approval integration test also exposed and verified an active-content precedence
fix. The review and implementation evidence are in `review.md` and `implementation/`.

## Decisions and limits

- Compact Pending loss was treated as Important because users could resend already queued
  input. The extra scope was one regression and its presentation fix; no runtime behavior changed.
- This round made zero paid model calls and makes no new Flash-quality claim. Offline fixtures
  cannot establish real-model reliability. The original 50 RMB ledger and the separate 5 RMB
  observation cap remain unchanged; prior conservative reservations of 1.628068 RMB remain held.
- Native Apple Terminal acceptance remains unverified: Computer Use previously rejected access
  to `com.apple.Terminal`. No AppleScript or other alternate route was used. Terminal-specific
  visual/input behavior is not ruled out by PTY checks.
- Release, signing, installation, package and documentation were verified by the implementing
  agent after code review; they did not receive a second independent delivery audit.
- The unchanged scheduler, permissions, model tools and legacy Ack application protocol were
  not re-audited as a new harness. Their regression suite ran, but unrelated pre-existing defects
  are not ruled out. A legacy Ack still proves persistence only, never application or completion.

No review findings were deferred. No remote push was performed.

## Package

`dist/terminal-redesign-2026-10-23/loh-macos-arm64-0.1.0.tar.gz` contains the signed executable,
updated tracked documentation, bundled Skills, brand assets, replay/cache helpers and a provenance
manifest. It excludes credentials, databases, provider logs and the user's untracked bug-review file.
Its sibling `.sha256` file verifies the archive.
