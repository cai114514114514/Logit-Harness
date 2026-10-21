# Evaluation executor implementation notes

The corpus is already frozen in `eval/frozen.json`; do not rewrite its tasks/oracle/protocol to fit
model outcomes. `loh eval freeze` and `status` work. Campaign execution and reporting are the next
implementation step; they are not yet available commands.

Implemented kernel interfaces:

- `Session.condition`: prompt, replan, tracking, dynamic (normal default), no_stagnation. Tests prove
  actual differences in command cancellation and independent completion. Replan waits for old jobs
  to settle, then resets the main context against the real current tree. It advances the dependency
  epoch and retires obsolete workers; it never resets source files.
- `Runner::observe(Arc<dyn Observer>)`: receives each persisted event before broadcast and the next
  model dispatch. A scripted user correction can therefore arrive exactly at a declared task event,
  instead of racing a later model request. The regular durable inbox remains the execution boundary.
- `Store::enqueue_once(session, key, value)`: transactional idempotent delivery; changing the content
  under an existing key is rejected. Ordinary client enqueue remains unchanged.
- `Config.user_skills`: true for normal sessions; false in the frozen study to keep personal skills
  out of all conditions equally. Project instructions/tools remain available.
- `ReportedFailure`: terminal usage survives truncated/invalid model output while tool execution is
  still refused. Meter and task budget settle actual reported usage. Missing usage stays uncertain.

## Next executor steps

Create a deterministic shuffled schedule of task × condition × three repetitions, development
first. There are 540 entries including the no-stagnation ablation; replan already supplies the
selective-repair ablation. Trial IDs and saved session IDs must remain stable across resume.
Freeze/copy the executable per stage and record its hash; do not silently mix binaries in a paired
comparison. Use the existing shared `artifacts/campaign/money.sqlite` account, never a new cap.

Prepare each workspace from the frozen fixture or a new project, with caller-owned NOTES.txt and
the initial public verification script from accept.py. Keep final verification text ready for the
scripted edit. Track initial implementation hashes. Use the event observer for first-write or live
verify.py-process triggers; record fallback delivery at completion/45 seconds. Persist an injection
intent before the caller file changes, enqueue with a unique correction key, then mark delivered.
Recovery must reconcile that intent: do not duplicate the user message or blindly overwrite later
changes. Record any agent modification to protected files before the caller's scripted replacement,
so that replacement cannot hide an earlier violation. Serialize receipt updates between daemon and
campaign driver. Preserve source/tool journals and confirm live owned process state on recovery.

Use the actual background runner and socket controls. A session-level optional evaluation spec can
install its observer in the daemon, preserving experiments when the driving terminal disconnects.
The driver resumes an existing trial, never creates a new billed duplicate on an observation timeout.
Wait for correction application and a subsequent completion before grading. Completed/failed/
exhausted artifacts still receive external checks. Respect the frozen rescue limit; record false
completion before feedback. Protect the total monetary limit even when task token limits differ.

The external oracle builds/runs actual code and compares JSON values and protected bytes. Its
functional score is not proof of every prose instruction (style/verification honesty have a separate
human rubric). Record observed public-check execution as an additional diagnostic. Keep human review
cells empty; neither this assistant nor the target model is a human reviewer.

Report paired task-level bootstrap intervals, all baseline comparisons, timing/cost, obsolete
actions, correction-trigger fallbacks, failures, and missing/uncertain measurements. Retain raw data.
Do not treat cancelled token estimates as measured waste, or an unfunded unfinished schedule as a
completed study. A provider stream without terminal usage remains conservatively charged; the
event-boundary injection avoids introducing an unrelated model-dispatch race into the benchmark.

Before freezing a holdout executable, finish the outstanding engineering watch list in progress.md
(notably current browser-origin authorization, assumptions/acceptance provenance, time accounting,
ignored-file evidence, and additional lifecycle edges). No holdout-driven harness tuning afterward.
