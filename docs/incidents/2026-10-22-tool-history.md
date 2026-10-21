# Tool history interrupted by a correction

## Summary

On 2026-10-22 at approximately 02:23–02:25 Beijing time, four `dev-decoder` development trajectories
failed with HTTP 400. Twelve rejected requests returned no usage. The original traces are retained;
their conservative 25.952256 RMB reservation remains uncertain, not asserted to be an actual charge.
The fixed payload subsequently succeeded through the same official Flash endpoint and shared meter.

## Reproduction and evidence

- Endpoint: `POST https://api.deepseek.com/responses`; bearer authentication from the environment.
  The key is absent from saved request payloads and this document.
- Representative request: session `382a2f46eb9a4396`, artifact
  `e9634b95-99e3-4061-bd49-fd7bc22fabc6.request.json` under its campaign logs.
- Representative provider rejection ID: `4b29c1a0-2abe-432f-881f-b442894a6413`.
- Response: HTTP 400, `invalid_request_error`, reporting no output for
  `call_00_RodwpsnikX9Slznc5cRg1068`.
- That output actually exists at item 38, after the call at item 36 and a user correction at item 37.
  Three retries preserved the invalid ordering and therefore could not repair it.
- The skill's HTTP replay script reproduced the ordering violation against a clearly labeled local
  fixture. Before/after fixture bodies and metadata are in `artifacts/protocol-replay`.
- The actual fixed request was replayed once with `examples/replay_probe.rs` through `Metered`.
  It returned model `deepseek-flash`, 20,790 input / 122 output tokens, including 384 cached input
  tokens. Configured-rate cost: 0.04180336 RMB. Its proposed tool call was **not executed**.

## Root cause and fix

The chronological transcript immediately appended a user interruption while an assistant tool batch
was unsettled. DeepSeek maps function calls into adjacent assistant messages, so the intervening user
message broke that grouping. This is consistent with the official
[Responses API input-item mapping](https://api-docs.deepseek.com/guides/responses_api/); the real failed
payload and successful reordered replay provide the direct evidence for this case.

`Context::request_items` now preserves raw chronological history while constructing the wire history
with user/developer interruptions after the complete tool batch. It preserves both messages and tool
results, rejects missing/orphan/duplicate outputs before dispatch, and also works with an already-saved
interleaved transcript. Requirement revision and cancellation are still applied immediately locally.

## Prevention and study treatment

Regression tests cover two interleaved user messages, parallel tool calls, actual cancellation results,
serialization/reload, orphan outputs and an unfinished batch. The runner uses this path before token
reservation and request persistence, including compaction requests.

The twenty development trials under `artifacts/evaluation/development` retain their original pinned
executable. Sixteen passed external acceptance, four failed due to this protocol defect. They are
development diagnostics, not evidence that a strategy outperforms another: replan discards its old
context and therefore avoided this particular defect. Do not silently replace the pinned binary or
rerun these IDs as if they were original observations. A repaired study must record a new executable
and directory while retaining the same frozen tasks and total monetary account. No holdout has run.
