# Whole-branch engineering review

The fresh reviewer examined `7759471`, including the initial domain/store code outside the diff
range. It reported four Important findings, no Critical finding and no deferred style findings.
The pre-fix assessment was **not ready for release**. The author reproduced each finding before
repair; the following fixes form one review-fix pass, verified with regressions rather than a
second review of the same changes.

| Finding | Repair | Regression evidence |
|---|---|---|
| A stopped daemon exits before CLI Resume arrives | Persist the idempotent control before startup and wait for its consumption; reconcile a dying old endpoint without adding another control | `cli_resume_persists_intent_before_starting_a_stopped_daemon`: failed with exit 1, then passed |
| Evaluator cannot finalize a stopped trial after a driver crash | Finalize a settled stopped state without starting a model process | `real_daemon_trial_grades_rescues_and_resumes_without_duplicate_model_calls`: failed trying to start a provider, then passed without another model call |
| Stdio cancellation erased uncertain remote MCP effects | Persist the concrete effect and retain External/Unknown receipts after local termination and restart; later config changes do not rewrite old provenance | `stdio_external_effect_stays_uncertain_after_cancellation_and_restart`: real localhost POST, failing missing receipt, then preserved receipt and blocked retry |
| TERM kills the leader while a child survives | Validate remaining members, escalate after the grace period and confirm exit; persist inherited ownership markers for leaderless recovery | `termination_kills_and_confirms_descendants_after_the_leader_exits`: reproduced live survivor, then passed; additional tests cover orphan recovery, normal command completion and mismatched PID identity |
| Authorization is an opaque hash with no reviewable action | Preserve the concrete tool and redacted arguments, render them and save a full private preview; hash the original action for authorization | `authorization_requeues_the_exact_pending_action_and_rechecks_live_scope`: failed generic tool name, then rendered command and required a fresh origin approval; separate credential-redaction test |

After the fixes, strict Clippy and the full development suite passed **126 tests**. Source-level
validation is separate from release-mode checks, package inspection and actual research acceptance.

## Rulings on behavior the reviewer declined to judge

- **Research effectiveness remains unestablished.** There is no holdout result or completed human
  rubric. Development results, including protocol failures, remain visible. Calling them proof of
  improvement would mislead the user; no such claim is made.
- **Native Apple Terminal visual review remains unavailable.** Computer Use denied access, and no
  alternate automation bypassed it. Actual PTY behavior is verified, but that does not establish
  native-window visual quality.
- **Arbitrary-shell adversarial containment is outside the approved policy boundary.** File patches,
  owned process groups and managed readonly services have concrete guards; an ordinary shell is not
  advertised as isolation from arbitrary hostile code. Unknown process ownership is refused, not
  guessed from a reused identifier.
- **Distribution notarization is not claimed.** Local macOS ARM64 packaging uses the built executable
  and verifies its ad-hoc signature. No Developer ID or public release publication was authorized.
  Installed behavior is checked separately before delivery.
- **The web probe and later progress notes were outside the reviewed HEAD.** The probe uses the same
  shared money ledger and was validated with actual Search/Fetch responses plus strict Clippy. It
  does not change product execution or establish research effectiveness.

No minor findings were deferred. The review and repair do not close the independent research gate.
