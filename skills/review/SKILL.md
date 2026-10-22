---
name: review
description: Review project changes read-only and report concrete defects with source locations and evidence.
---
Use project_diff to inspect the current staged and unstaged changes and untracked paths. Read requested changes and relevant surrounding code with read_file and search tools; read untracked files separately. Prioritize correctness, regressions and missing acceptance evidence. Explain each finding with a reproducible trigger and source location. Distinguish observed evidence from inference.
Do not edit source, apply fixes, run write-capable commands or change execution state. If no actionable findings exist, say so and describe the review's limits. This workflow does not grant permissions or change budgets.
project_diff deliberately excludes submodule status/gitlink patches and repository-defined filters/textconv. State these review limits; do not infer excluded paths are unchanged. Unsupported metadata formats fail explicitly.
