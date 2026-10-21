# Preregistered steering experiment

This is an authored, controlled corpus, not SWE-bench or a representative sample of all software
engineering. It measures adaptation to a specified correction while preserving unaffected behavior.
Every task produces a real runnable JSON-lines CLI. Each language has four development and eight
held-out tasks; each split is half greenfield and half existing code. Existing fixtures contain
working surrounding code and at least one defect. No paid trajectory on this corpus may run before
`loh eval freeze` records every task, fixture, acceptance script and protocol file by hash.

## Conditions

All conditions share the same provider alias, thinking setting, budgets, tool definitions, write
serialization, file-hash protections, permissions, process ownership, two-worker capacity and test
tools. Conditions vary only the following execution mechanisms:

- **prompt**: corrections are appended to contexts; no persistent requirement revision or independent
  completion model. The normal loop asks for implementation/testing, then a final response ends the
  attempt once all tools/workers settle. External acceptance can expose a false completion.
- **replan**: persistent requirements and independent verification; each correction invalidates all
  dependency scopes/evidence, cancels active work, retires old contexts and restarts planning from
  the current tree. It does not undo existing files or overwrite concurrent user edits.
- **tracking**: persistent requirements and fresh evidence; obsolete queued actions are rejected,
  but a correction does not cancel already-running tools. Their old evidence cannot certify the
  revised goal. This isolates the added benefit of coupling changes to active execution.
- **dynamic**: the full mechanism, retaining unaffected scopes and cancelling affected work before
  replacement mutations, with fresh independent acceptance.
- **no_stagnation**: dynamic with repeated-no-information diagnosis disabled.

Removing selective repair gives the **replan** condition, so its paired trajectories are reused as
that ablation, not rerun and misrepresented as independent observations. All other settings remain
fixed. Worker count/call count are reported; no improvement is attributed to extra concurrency.

## Tasks and intervention script

The agent receives the task specification, CLI contract and two public examples. The caller owns
`verify.py` and `NOTES.txt`; the agent may read them but must not modify them. `verify.py` checks the
public examples and includes a disclosed three-second simulated CI delay. The external acceptance
program checks additional frozen inputs, about-command behavior and protected-file hashes.

Half of the tasks inject a correction after the first successful implementation change. The other
half inject it after implementation exists and `verify.py` is executing. If that trigger never
occurs, inject at the first completion attempt or after 45 seconds, and report the fallback; do not
silently drop the task. The same correction also updates the caller-owned public verification file.
This is a controlled latency/user-edit scenario, not a claim about natural CI timing.

Every trial gets the same one mandatory correction. After a false completion, the evaluator may
send at most two standardized acceptance-failure messages. A 30-second period with no active model
or tool and no terminal status receives the same bounded continuation prompt. These are **scripted
interventions**, not observations of human behavior. Actual human review/time remains separate.

## Schedule, cost and recovery

Interleave conditions within each task/repetition using the fixed seed, run development before
holdout, and save each trial's source hash, binary hash, actual model response metadata, event stream,
model/tool artifacts, external check results and cost. Development findings may change the harness
before a separately recorded holdout binary freeze. Once holdout starts, do not tune against its
outcomes. Infrastructure/protocol defects require an explicit amendment preserving old runs.

The 50 RMB account includes earlier engineering probes. Every role and web request uses that same
account. No account reset, automatic funding, duplicate interrupted mutation or unmetered provider
request is allowed. A budget-limited campaign stays incomplete and resumable. An interrupted trial
reconciles its original session and files; it is not replaced with a fresh billed attempt.

## Fixed scoring

Requirement fidelity is the mean of four groups: stable core behavior, the correction, edge cases,
and preserved about/file behavior. A group passes only when all its checks pass. External acceptance
passes only when every check passes. A missing/unrunnable deliverable fails its functional checks.
Record false completion before rescue, obsolete action finishes, time from correction delivery to
applied revision/next useful action, cancelled/wasted work, tokens and monotonic elapsed time.
Cancelled work is a proxy; do not call all cancelled tokens objectively useless. Report fallback
triggers, missing measurements, protocol errors and blocked/exhausted trials separately.

For paired uncertainty, average repeats within each task, then bootstrap task IDs (10,000 resamples,
fixed seed). Select the strongest of prompt/replan/tracking by held-out external acceptance, then
fidelity as a tie-break; also report all three paired comparisons to expose selection uncertainty.
The research gate is fidelity no worse than five percentage points and at least 25% reduction in
scripted interventions or measured wasted work. Confidence intervals must support the direction;
small samples or mixed outcomes are inconclusive. Engineering checks are a separate gate.

Human rubric (0, 1, 2 each): readable responsibility-focused modules; useful comments explaining
non-obvious decisions; faithful handling of the correction without weakening the goal; honest
reporting of remaining limitations. Export a blinded review sheet and identify the actual reviewer.
Leave cells empty until a person reviews them. Automated checks or another model do not fill them.
