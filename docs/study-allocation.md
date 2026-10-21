# Budget allocation before held-out execution

Recorded on 2026-10-22, before any held-out trajectory. This is an allocation amendment, not a
change to task inputs, conditions, scoring, thresholds or the 50 RMB authorization.

The original development stage retains 20 finished trajectories out of 180: 15 interval trials
and five decoder trials. Four decoder trajectories encountered the now-reproduced tool-history
protocol defect. Their original executable, source, outputs and costs are retained; they must not
be silently pooled with a repaired binary or described as 20 distinct tasks. The remaining 160
development entries are unrun, not deleted or counted as passing.

The reviewed engineering release passed the complete 126-test suite and 20 release-mode
integration tests, a real replay of the rejected provider request, and real Search/Fetch calls.
The bounded installed-binary smoke in `artifacts/release-smoke` completed, but its initial oracle
leaked future constraints; it does not prove storage adaptation. It also exposed contradictory
verifier evidence causing needless implementation work. That engineering repair passed the full
127-test suite. The corrected `artifacts/storage-correction` fixture must settle before held-out
work starts. Both are separate engineering probes with no rescue prompts, not research trials.
All outcomes and further defects are recorded before the held-out binary is pinned.

The shared account had 10.37972104 RMB of configured-rate usage and 25.952256 RMB of uncertain
HTTP 400 allowances before that smoke. Provider billing evidence has not been received; uncertain
allowances are not released on an assumption. The authorized cap remains 50 RMB, including the
smoke and all previous experiments. This leaves insufficient established funding to promise the
remaining 520 research trajectories.

Allocation decision: close this development execution stage as **incomplete**, then allocate the
remaining admissible balance to the separately pinned held-out stage. This departs from completing
all development repetitions before holdout, and is disclosed rather than hidden. Use the existing
registered deterministic holdout order, all five conditions, three planned repetitions, equal
500,000-token / 180-second / 8,192-output limits, two workers per agent and two trial jobs. No
post-outcome selection, replacement account, changed task oracle or new harness tuning is allowed.
The automatic account guard may leave the final maximum-request reserve unused. If a new protocol
defect appears, stop the stage and retain it; do not repair and mix executables inside this stage.

Initial candidate provenance (superseded by the verification feedback repair before holdout):

- Corpus digest: `2e710f9e3af45a70edec4654b3fb9827e19b770e455894a4c2e652ae2267dbbf`.
- Source hash: `3e50639ce50ee15d3e5c4cd0a32506b3236d112b549ead8970b4d59b1849f933`.
- Executable SHA-256: `197dfdfd8773519e7330405263dc1cf8951c8e9e0bcf77e5a0643347cedb8b36`.
- Account: `weft-first-evaluation`, original `artifacts/campaign` state directory.

The repaired candidate is commit `cec7d6b`, with 127 development tests and 21 release-mode tests
passing, strict Clippy passing and a clean default release build:

- Source hash: `a55af6d616bf4ca44337cdd4f61953788cb2ffd52de4a68b85cb12564a78fddf`.
- Executable SHA-256: `debad6356f7ca9fc44e0269e3a7f17385713806e9c268d67132155cefebd6ef9`.
- Installed at `/opt/homebrew/bin/loh`, verified byte-identical to the built executable and with a
  valid local ad-hoc signature. The storage-correction probe records this exact executable hash.

The study's generated `study.json` is the exact execution pin. All 360 held-out entries remain in
its denominator even if funding stops earlier. Reports retain task counts, missing repetitions,
errors, scripted interventions, conservative costs and uncertainty. Missing human review remains
empty. A partial study cannot establish the registered research gate; an isolated successful
storage-migration example cannot establish general superiority either.

## Final pre-holdout pin

The second probe exposed a separate acceptance-scope defect: `replace` combined with updates could
deactivate the just-updated core requirement. That defect is preserved and documented in progress.md.
Commit `b4b4099` rejects this contradictory delta atomically and preserves the verbatim new objective
for valid whole-task replacements. The previous two candidate hashes above are historical, not the
held-out executable.

The final candidate passed 130 development tests, 36 release-mode tests, strict Clippy and a default
release build. Real session `e2cb3548e7ef441d` then passed the corrected storage acceptance: initial
JSON behavior verified; a live old check cancelled; core requirement retained at revision 2; SQLite
data migration and caller-file preservation verified; independent completion reached. Its records
are in `artifacts/storage-acceptance`. It used 261,397 tokens / 134 active seconds and 0.33823928 RMB
at configured rates. This is one engineering example, not comparative effectiveness evidence.

- Source hash: `1c16dc63d232a4e91ed4cefb06dd68dbc61b237d817c4c5ace32f9e068cd3ba7`.
- Executable SHA-256: `41242b0b92eae3a9c3103099e088cebfd86cb27922781020f60134ad4827533d`.
- Ledger immediately before held-out execution: 11.89430656 RMB accounted, 25.952256 RMB uncertain,
  zero reserved; 12.15343744 RMB remains before maximum-request admission reserves.
- No model receipt supplies billing evidence for the uncertain calls; no allowance is refunded.
- No held-out trajectory had run when this pin and allocation were recorded.
