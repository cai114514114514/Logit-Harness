# Context overflow from workspace-wide fingerprints — 2026-10-26

## What the user saw

Sessions paused with `provider request stopped; progress is saved. 请根据错误详情修正请求格式或参数，
再用 /resume 继续。` and did so again after `/resume`. Two sessions were affected:
`36b8823bfb75499c` (2026-10-26) and `50bd10c1086f42fa` (2026-10-23).

## Cause

The provider rejected the request with HTTP 400 for context length: 1,297,822 and 3,779,858
tokens against a 1,048,576-token window. In `36b8823bfb75499c` the whole history was two user
messages and one `project_diff` call:

- `project_diff` fingerprints every workspace file, including ignored `artifacts/` (8,373 files)
  and `dist/` (382). Its result carried all 9,197 fingerprints into the model context (1.4 MB).
- The same map entered `scope.observed`, and the next state update sent it again (1.4 MB).
- The history now exceeded `context_limit`, so the runtime asked for a summary, but the summary
  request carried the complete history and overflowed itself.
- A context-length 400 was classified like a malformed request: the session paused, and
  `/resume` resent the same history.

## Fix

- `324ac91` The model sees fingerprint maps of at most 128 files in full and a count beyond that,
  in tool results and in state. Evidence and the harness scope keep every fingerprint.
- `19276e0` Context-length rejections compact and retry without pausing. Summary requests are
  bounded (oversized items, then the largest, become omission notes; tool pairing is kept) and
  shrink by half on each overflow, stopping with a stated reason after three retries.

## Verification

- Regression: 420 passed, formatting, strict Clippy, release build. New tests reproduce the 9,197-file
  map in state and in a summary request, and drive the runner through an overflow (recovers without
  pausing) and through overflowing summaries (shrinks, then stops with the reason).
- Read-only estimate on the affected sessions: summary requests become about 21 KB and 9 KB.
- Installed as `/opt/homebrew/bin/loh`, source commit `19276e020dfb4644a92619d58847890fe4ea5ff5`,
  source hash `7719c9f656dd8764bd41dd191f7f66a7241d3198b0af3dcbedbcf28eecb1e042`, signed binary
  SHA-256 `7d496e75f470e02d89cc971962dfcfdcfa49d5485949f0aa6473956a7c145b24`; all 131 pre-existing
  processes remained. Installed PTY suites passed; evidence is `artifacts/context-overflow-2026-10-26`.
- Not verified against the real provider: no paid request was made. The affected sessions recover
  with `/resume`, which first sends a bounded summary request.
