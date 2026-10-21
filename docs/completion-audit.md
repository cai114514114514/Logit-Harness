# Full-scope completion audit

Audit date: 2026-10-22. The complete objective is **not achieved**: the registered research schedule
and human review remain incomplete. This audit maps the approved plan to concrete evidence; it does
not replace the plan with the subset that happens to pass.

The worktree's execution core is unchanged from `b4b4099`. Installed, built and held-out binaries
share SHA-256 `41242b0b92eae3a9c3103099e088cebfd86cb27922781020f60134ad4827533d`; source hash
`1c16dc63d232a4e91ed4cefb06dd68dbc61b237d817c4c5ace32f9e068cd3ba7`.

## Product and mechanism

| Approved requirement | Authoritative implementation and observed evidence | Assessment |
|---|---|---|
| Independent Rust CLI; macOS; product Logit- Harness, command loh | Cargo.toml; src/cli/args.rs; installed `loh --version`; dist/delivery/verification.json | Engineering verified |
| New projects and existing projects | Same Workspace/Runner path; actual create/replace/delete tests; greenfield holdout external checks; existing-project storage-acceptance probe | Both paths exercised; general task reliability remains unproved |
| Continue until actual completion, accepting interruption | runtime/runner, control, steering, verify; early-done and side-question regressions; storage-acceptance reaches independent Completed | Mechanism verified; not a guarantee all model tasks finish |
| Background work survives client closure and reconnects | runtime/daemon.rs, ipc.rs, store.rs; detached_cli, actual_pty, cursor replay and crash recovery tests | Real subprocess/socket/PTY evidence |
| User-source requirements, assumptions and acceptance notes remain distinct | domain/goal.rs, notes.rs; exact-source extraction, atomic revision, annotation provenance tests; final probe retains active r1 | Engineering verified |
| Link requirements, steps, actions, artifacts and acceptance | runtime/graph.rs, actor.rs, tools/host.rs, domain/evidence.rs; transitive invalidation, late patch, stale checks/build-artifact tests | Engineering verified; unknown dependencies conservatively broaden invalidation |
| Questions, refinements, replacement, clarification and nudges differ | runtime/steering.rs, questions.rs; paused/completed side-question and independent-work tests; contradictory replacement retries | Engineering verified; semantic classification still uses the model |
| Cancel affected active work, confirm exit and protect edits | tools/process.rs, group.rs, identity.rs, edit.rs; stubborn descendant/orphan recovery, hash rejection and external-edit tests; real old-check cancellation | Engineering verified; ordinary shell is not arbitrary-code isolation |
| Retain unaffected work and repair current files | graph invalidation and versioned worker leases; independent storage/UI tests; protected caller hashes in actual correction probe | Engineering verified; no wholesale automatic reset |
| Detect no-information failure loops; wait for live work/input appropriately | runtime/progress.rs, questions.rs, clock.rs; diagnosis and independent-work regressions | Engineering verified |
| Model response end is not task completion | runtime/verify.rs; omitted/unsupported/stale evidence regressions; contradictory verdict repaired by verifier | Engineering verified; research exhaustion is separately reported |

## Interfaces, integrations and ownership

| Approved requirement | Authoritative evidence | Assessment |
|---|---|---|
| run --json, sessions, attach, pause, resume, stop, doctor, eval | cli/args.rs, commands.rs; CLI/daemon tests and installed doctor/version | Implemented and exercised |
| /status, /diff, /budget, /pause, /resume, /stop, /detach; Ctrl-C pause | cli/terminal/actions.rs, input.rs; command/parser and actual PTY tests | Implemented; /diff uses the Git worktree; native-window appearance unverified |
| Persistent sequenced events, process/cost/evidence state, crash reconciliation | store.rs WAL transactions; runtime/recovery.rs; process identity and unknown-write tests | Engineering verified; uncertain mutations are not blindly replayed |
| Main plus two independent workers; one mutation owner | runtime/workers.rs; barrier-based concurrent-context test with real file proposals and owner commits | Concurrent execution proved with scripted provider; not a measured quality benefit |
| Source revision/hash guards; command-long write ownership; managed services | tools/host.rs, edit.rs, services; actual service cannot write source but build output allowed | Engineering verified on macOS |
| Fresh independent acceptance context | runtime/runner.rs, verify.rs; fixture tests and actual storage-acceptance verifier trace | Engineering verified |
| DeepSeek Flash Responses, thinking, complete streamed arguments/history | model/deepseek.rs, stream.rs, context.rs; fragmented/truncated SSE tests; metered live protocol replay | Real provider evidence plus failure-path fixtures |
| File search/read/patch, terminal and process management | tools builtins/search/process/services; real temporary files and PTY/process tests | Engineering verified |
| Separate DeepSeek Search/Fetch | tools/web.rs and quota; artifacts/web-live with two search results and fetched document | Real official calls verified |
| Chromiumoxide profile, operation, DOM/AX, screenshot, console | tools/browser modules; real Chrome input/click/origin tests; browser-proof and vision-probe records | Actual browser and model-image evidence |
| Official MCP SDK, stdio and Streamable HTTP | rmcp dependency; extensions/mcp; actual child and localhost HTTP SDK tests, cancellation and uncertain-effect receipts | Both transports exercised; remote unknown outcomes remain blocked |
| Project/user SKILL.md on demand | extensions/skills and knowledge; metadata-only discovery, selected-resource loading, scoped instructions and stale skill tests | Filesystem-backed integration verified |
| Local automatic work; scoped external authorization at tool entry | tools/policy.rs, host.rs, approval.rs; origin-bound retry and redacted concrete approval tests | Engineering verified within documented application-policy boundary |
| Defaults: Flash, 6 hours, 5M combined tokens, 2 workers | Config::default; runtime/dispatch and budget; shared concurrent reservation tests | Matches approved defaults |
| Search quota, explicit-rate costs, env-only credential and resume limits | WebUsage persistence; runtime/money; DEEPSEEK_API_KEY loader; control Budget; actual shared 50 RMB guard | Implemented; normal config contains no key |

## Engineering failure coverage

The saved `artifacts/correction-scope/tests.log` contains 130 passing tests and no failing result;
`release-tests.log` contains 36 passing release-mode tests. Source inspection confirms the tests
exercise the stated behaviors, including actual processes, sockets, Chrome and filesystem writes;
provider responses in failure-path tests are explicitly fixtures.

Coverage includes incomplete/malformed streams, context compaction, request cancellation, token and
money exhaustion, stale worker/queued actions, simultaneous caller edits, live services, lost clients,
daemon crashes, omitted requirements, stale evidence and missing required user input. This audit
found a missing dedicated 429 Retry-After transport regression and added `tests/rate_limit.rs`.
Its four real-localhost HTTP checks now pass: two retries preserve the complete request and report
attempts; persistent 429 stops after three attempts; cancellation interrupts backoff; malformed delay
uses a nonzero fallback. Targeted strict Clippy passes. Logs: `artifacts/completion-audit`.
These four checks add failure-path evidence without changing the execution core or contacting the
paid provider. No real production rate-limit incident is claimed as acceptance evidence.

## Research and delivery

| Approved requirement | Current evidence | Assessment |
|---|---|---|
| Fixed references and testable mechanism difference | docs/spec.md reference list and novelty boundary; preregistered corpus/protocol and allocation amendment | Hypothesis defined; novelty/effectiveness not proved |
| 12 development + 24 held-out tasks, balanced languages/project kinds, 3 repetitions | eval/registered.json, manifest validation and deterministic schedule test | Corpus and schedule implemented |
| Four conditions plus local-repair/stagnation ablations | runtime/condition.rs; executable cancellation/verification/diagnosis tests; replan is selective-repair removal | Conditions implemented; no duplicate ablation presented as independent |
| Frozen scoring, paired uncertainty, real trajectories and preserved failures | eval/grade/stats/gate; report/campaign tests; separate pinned studies and per-trial artifacts | Evaluation infrastructure verified |
| Execute complete real comparison | 20/180 development, 15/360 holdout; 505 remaining | **Incomplete: money admission blocked** |
| Actual human review and intervention/waste evidence | Empty human-review.csv; scripted interventions explicitly labeled; measured useless work absent | **Incomplete; automated/model review is not substituted** |
| Research threshold: fidelity margin and 25% reduction | One paired held-out task; no task-level CI; zero scripted-intervention reduction against strongest observed baseline | **Not established** |
| macOS executable, docs and replayable records | Installed binary; dist/delivery archive/manifest/verification; scripts/replay.py; synthetic event exports | Delivered and extracted-package replay verified |

## Remaining actions and boundaries

The prior goal turn made substantive progress (repairs, tests, real runs, package and report). This
continuation verified the same financial blocker: 19.76413632 RMB accounted plus 28.114944 RMB
uncertain, zero reserved; 2.12091968 RMB available versus 2.162688 RMB required by the frozen next-call
reserve. No live evaluation daemon remains. This is not a wait on a running job.

`artifacts/campaign/billing-review.json` exports the 13 uncertain charges and matching-session
failure receipts without keys or request bodies. There are 12 provider request IDs for HTTP 400
responses and one network-send failure without a provider ID. The ledger does not persist an exact
charge-to-request foreign key, so the export does not invent such a mapping or zero any charge.

The discovered offline failure-path check is now complete. The frozen experiment program remains
unchanged. Completing paid trials needs verified billing
reconciliation or a user-authorized budget change, and the human rubric needs an actual reviewer.
Do not silently lower limits, remove tasks, reset an account, claim the missing research passed, or
fill human-review cells using a model.
