# Pre-holdout execution-limit amendment

The first five development trials (dev-intervals, repetition 1, every condition) used a 100,000-token
limit and 4,096 output tokens. All exhausted before producing a runnable CLI. Actual settled usage
was 50,607–70,394 tokens: conservative admission could not reserve the next request. Two trajectories
also returned explicit incomplete terminal responses. Rust trajectories began implementing JSON
parsers despite serde_json already being available offline, which was not stated in their context.

Before any held-out trial, apply these settings equally to all conditions: 500,000 combined tokens
and 8,192 output tokens. The elapsed-time limit remains 180 seconds. The common Rust task context now
states the verified availability of serde_json 1.x in the local offline cache. Functional requirements,
private/public check inputs and outputs, scoring groups, intervention triggers, repetitions, condition
definitions, seed, uncertainty method and research thresholds do not change. The overall 50 RMB cap
does not change. Larger per-task ceilings do not authorize spending beyond that account.
This is a combined execution-limit/environment amendment; differences from the pilot do not isolate
the causal contribution of any one of these changes. Primary comparisons use the amended settings
equally across all conditions.

Keep the five failed trials as execution-limit pilot evidence, not as primary comparative results.
Their original corpus and manifest are archived at artifacts/evaluation/pilot/{corpus,frozen.json};
the original committed freeze remains eval/frozen.json with digest
c19d6137492f5401bba7a8223d5045d94da620e4755537c32034f27c2851fd4c.
Register the amended corpus under eval/registered.json and start a separate development run, using
the same monetary ledger. No held-out outcome has been observed or used to make this change.
