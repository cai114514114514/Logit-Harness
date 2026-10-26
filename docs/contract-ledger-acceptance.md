# Contract-ledger terminal delivery

Implemented 2026-10-26 in the working tree, from the
[design draft](superpowers/specs/2026-10-26-contract-ledger.html) and its
[specification](superpowers/specs/2026-10-26-contract-ledger-design.md). The user confirmed the
draft's four open decisions the same day: the names 契约/条款, a darker accent on light
terminals via a one-time OSC 11 query, clause tags on edit/run/check lines only, and additive-only
IPC changes. Run `loh` in a new terminal to use the installed client; existing clients and
background sessions were not restarted and keep running the previous binary until they exit.

## Delivered behavior

- Runtime: new `Contract` and `Acceptance` events, written where revisions and verdicts already
  happen (`meta.rs`, `steering.rs`, `questions.rs`, `team/corrections.rs`, `verify.rs`). A
  revision record lists what that revision actually did: changed, added and retired clauses,
  cancelled actions and whether they had settled, evidence that stopped being current and why,
  and kept steps. `SessionView.contract` derives clause stamps from the same goal, evidence and
  step versions as the completion gate; `Session.acceptance` keeps the latest verdict. All new
  fields have serde defaults; old sessions replay with the previous one-line notices.
- Patch results carry exact `added`/`removed` line counts (Myers on the committed contents);
  differences too large to count carry none.
- Scrollback: speaker bars (▌ you, │ main, ┃ member, ║ verifier) replace heading lines; tool
  lines use Chinese verbs with measured results, elapsed time and served clauses; the contract
  card, revision cards and acceptance receipts are written once.
- Active region: the contract bar, one-row running status, one rule above the editor naming mode
  and state, `▸` prompt, footer with shortcuts left and model/usage/KV right, narrow collapse.
- `/contract`: a local full-screen page with every clause's source, revisions, evidence, verdict
  and running work; it falls back to the latest archived goal after verified completion.
- Theme: colour tokens; OSC 11 + DA1 background query before input starts; `NO_COLOR` strips
  colour at both output points.

## Exact installed version

| Property | Value |
|---|---|
| Executable | `/opt/homebrew/bin/loh` |
| Version | `0.1.0` |
| Branch | `contract-ledger` |
| Source commit | `13e7bd982fa34684d8169f74b614cecb67dd7253` |
| Source hash | `64df605f46012a720bf6ba5234de472b8b89f189e78dbd062bdb2b99b5d2a388` |
| Signed binary SHA-256 | `1b745330549666dd319070a4f52832210751c170aa8e1bb95cc03a156fc7c714` |
| Installation | Ad-hoc signed, strict signature verification, atomic replacement |
| Previous binary | `artifacts/contract-ledger-2026-10-26/previous/loh`, SHA-256 `36e3dd0b…137e78` |
| Process preservation | All 123 `loh` processes present before replacement remained afterward |

The receipt is `artifacts/contract-ledger-2026-10-26/install.json`. Documentation commits after
the source commit do not change the compiled source hash.

## Deliberate differences from the draft

1. Committed history is laid out at width − 1 so no terminal auto-wraps a full row; card edges
   are one cell shorter than in the draft.
2. The main view of a team still hides member tool streams, as confirmed in the 10-23 design.
   The draft's frame 5 shows them; the main view instead shows their effect through the contract
   bar and member wait status. This needs the user's decision before changing.
3. After verified completion the runtime archives the goal and returns to conversation mode, so
   the bar disappears and the rule reads `对话 · 等待输入`, not the draft's `Engineering · 已完成`.
   The receipt stays in history and `/contract` shows the archived contract.
4. At 40 columns four clauses still fit with their numbers (`契约 v2  ○r1 ○r2 ⊘r3 ○r4`); the
   stamps-only form is used only when they do not fit.
5. Receipt headers say `重跑 N 组检查`, counting the verifier's checks and commands, rather than
   claiming they were tests.

## Verification

Evidence paths are relative to `artifacts/contract-ledger-2026-10-26/`.

| Layer | Result | Evidence |
|---|---|---|
| Rust regression | `cargo test --locked`: **414 passed, 0 failed, 0 ignored**, 60 result groups | — |
| Formatting and lint | `cargo fmt --check`; `cargo clippy --locked --all-targets -- -D warnings` | — |
| Release build | `cargo build --locked --release` | `target/release/loh` SHA-256 `9c9fe049…74511` |
| Stamp rules | 13 unit tests in `src/runtime/contract/tests.rs`, including the event projection agreeing with the next derived view | — |
| Runtime events | `tests/steering.rs` (revision record), `tests/verification.rs` (verdict and stamps) | — |
| Whole screens | `tests/contract_screens.rs`: frames 1–8 as character grids, 60×18 and 40×12 | — |
| Real PTY, offline scripted provider | 8 runs of `tests/pty/contract.py`: 110×34, 80×24, 60×18, 40×12, light, `NO_COLOR`, rejection at 80×24 and 60×18 | one directory per run |
| Existing PTY suites | editor, layout, interaction, team, limits all passed against the same debug binary | `suites/` |
| Installed binary | contract 80×24, 40×12 and rejection, plus all five existing suites, passed against `/opt/homebrew/bin/loh`; receipts name the installed SHA-256 | `installed/` |

The PTY runs used `target/debug/loh`, SHA-256
`5bee1a6e7533ceee26995cb37851777effa6f1cd101ca4241610cca35b0155aa`.
In the captured ANSI streams the default accent `38;2;91;156;255` appears 229 times on a dark
background and never on a light one, where `38;2;0;92;197` appears instead; the `NO_COLOR` run
contains no foreground colour sequences at all.

Reproduce one run with a Python that has `pexpect`, `pyte` and `Pillow`:

```
python3 tests/pty/contract.py --size 80x24 [--light] [--no-color] [--reject]
```

## Limits

- Every model turn in the PTY runs is scripted. They show presentation and runtime bookkeeping,
  not real Flash behavior; no paid request was made and both budget ledgers are unchanged.
- Grids come from pyte in a PTY, not from Apple Terminal. Native-window acceptance remains
  unverified under the earlier safety denial.
- The bar's step-invalidation effects on other clauses appear at the next two-second view
  refresh; contract and verdict events update it immediately.
