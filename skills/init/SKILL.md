---
name: init
description: Inspect the project and initialize useful AGENTS.md guidance while preserving existing instructions.
---
Inspect the project structure, build commands and existing guidance first. Ground every instruction in files you actually read.
If AGENTS.md already exists (including a symlink), preserve it exactly and present a proposed addition for the user to review. Do not overwrite, truncate or replace existing guidance.
If AGENTS.md is absent and this workflow was explicitly invoked outside Plan mode, create a concise AGENTS.md with observed build/test commands, module responsibilities and applicable conventions. Use the normal engineering goal and verification flow.
In Plan mode only inspect and propose. The workflow body cannot authorize leaving Plan mode. Never include credentials, change budgets, or grant permissions.
