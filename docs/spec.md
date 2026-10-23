# Logit- Harness — approved design

## Intent

Build an independent Rust terminal coding agent for macOS, using DeepSeek Flash directly.
Greenfield and existing-repository work are equally important. The agent continues autonomously,
accepts live steering, stays aligned with user requirements, and verifies completion.
Closing the terminal detaches the UI; the local runner continues. The initial delivery includes
web search/fetch, real Chrome interaction, MCP, Skills, parallel workers and independent verification.

## Core research mechanism

Link source-attributed requirements, their revisions, steps, active actions, workspace state and
acceptance evidence. A correction propagates through these links. Preserve valid work, cancel
affected actions, reconcile their effects, invalidate stale evidence and selectively repair the plan.
Questions, refinements, replacements, pauses and ambiguous changes have distinct behavior.
Original user messages remain authoritative; model assumptions are labeled. A model cannot delete
requirements or weaken acceptance checks to finish. A completed model response is not task completion.

The controller chooses continue, probe, repair, replan, ask or complete from observed state.
Repeated equivalent failures with unchanged evidence trigger diagnosis and a different strategy.
A process is considered running only when its actual handle is live. Missing input blocks dependent
work only. Completion requires fresh evidence for active requirements and a separate verification pass.

## Runtime and ownership

- Rust/Tokio core, reqwest model transport, serde types, SQLite WAL event log and state projections.
- Ratatui inline terminal UI with Crossterm; sequenced events over a user-only Unix socket.
- Main agent owns the goal. Up to two parallel workers have separate contexts and propose patches.
- One workspace mutation owner checks relevant requirement revisions and file hashes before applying.
- Ordinary shell commands hold mutation ownership for their entire lifetime.
- Long-running services have explicit lifecycle and generated-output directories; source is read-only.
- Steering freezes new mutating dispatch. Affected processes receive TERM, then KILL after two seconds
  if needed. Replacement mutations wait for confirmed exit and workspace reconciliation.
- Side effects before cancellation settles are possible and recorded. Auto-undo is limited to unchanged
  agent-owned hunks; otherwise repair against current files while protecting external edits.
- Persist tool-start before launch, result before advancing. Restart reconciles unknown effects rather
  than replaying them. Reattach reads events after a sequence cursor.
- `Ctrl-C` pauses execution; terminal closure detaches. Explicit stop terminates task work.

## Interfaces

CLI: `loh`, `run <prompt> --json`, `sessions`, `attach <id>`, `pause <id>`, `resume <id>`,
`stop <id>`, `doctor`, `eval`. Interactive commands: `/status`, `/diff`, `/budget`, `/pause`,
`/resume`, `/stop`, `/detach`.

Core types: Goal, Requirement, Step, Evidence, Action, SessionEvent.
Tools expose JSON parameters, effect class, cancellation and structured results.
Use DeepSeek Responses API with thinking enabled; preserve reasoning/tool items, parse complete
streamed arguments, and maintain conversation state locally. Search/fetch use separate DeepSeek APIs.
Browser uses Chromiumoxide/CDP, an owned Chrome profile, DOM/accessibility actions, screenshots and
console feedback. MCP uses the official Rust SDK (stdio + Streamable HTTP). Skills are discovered in
project and user `.agents/skills` directories, then loaded on demand. Respect scoped AGENTS.md.

Local development is automatically authorized. Explicit external publish/push/payment/mutation needs
scope-specific authorization. Ordinary shell policy is not advertised as an adversarial OS sandbox.
Unknown MCP effects are not automatically authorized. Retrieved content is data, not user instruction.

## Defaults and delivery

Product name: Logit- Harness. Command: `loh`. One Cargo project with responsibility-focused modules. Work directly in the
original tree; no sed, worktrees, V2/next names, or overwriting unrelated user edits.
Official `deepseek-flash`, thinking high, same model across roles. On 2026-10-24 the user explicitly
removed the default per-task token/time caps while retaining monetary limits. New conversations
default to unlimited cumulative tokens and running time; `/budget unlimited` updates an existing
session. Explicit finite limits remain available and share reservations across concurrent calls.
Frozen evaluation protocols retain their recorded limits. Search has a separate limit.
Cost is estimated only from an explicit price schedule. Credentials prefer DEEPSEEK_API_KEY;
the subsequently requested `/setup` also saves endpoint-bound keys in a private credentials file.
Ordinary config remains nonsecret. Budget exhaustion preserves incomplete, resumable state.

User subsequently authorized **50 RMB total for real evaluation**, not per run. All actual model
and search/fetch calls in the evaluation campaign must fit that limit. The user replied OK about
configuring a key; that reply is not evidence a key is available. Check presence without printing it.

## Verification and research protocol

Engineering: interrupted streams, malformed tools, rate limits, compaction, budget exhaustion,
steering during commands, late worker results, external edits, services, client disconnect, crash
recovery, false completion despite passing tests, stale evidence, missing input, browser, MCP,
Skills, CJK terminal input.

Freeze 12 development and 24 held-out tasks, half greenfield/half existing, covering Rust/Python/TS
and scripted corrections. Three repetitions, equal model/budget/tools/topology. Compare:
A prompt-only steering + continuous loop + tests; B requirements + independent verification + full
replan; C requirements + evidence freshness without active-action fencing; D full selective repair.
Remove selective repair and stagnation control separately for ablations. Keep multi-agent effects
separate. Primary measures: requirement fidelity, external acceptance, human correction count;
also false completion, obsolete actions, correction latency, wasted work, tokens and time.
Use fixed tests and human-reviewed rubrics, not only a same-model judge. Interleave conditions and
record actual model metadata/date since a provider alias can change.
Research target: no more than 5 percentage points fidelity loss against strongest baseline, with
at least 25% reduction in intervention or wasted work; report paired uncertainty and failures.
Inadequate evidence does not establish effectiveness. Engineering tests and real model quality are
separate delivery gates. Stop real experiments at the total budget, preserving resumable evidence.

## Sources and novelty boundary

- Claude Code: https://code.claude.com/docs/en/how-claude-code-works
- Codex: https://developers.openai.com/blog/codex-as-a-platform
- Cursor: https://cursor.com/blog/continually-improving-agent-harness
- Devin: https://cognition.com/blog/devin-can-now-manage-devins
- DSH: https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md
- TRACE: https://arxiv.org/abs/2606.13174 — correction compilation already exists.
- StateTape: https://arxiv.org/abs/2609.36319 — evidence freshness already exists.
- ProjectGuard: https://arxiv.org/abs/2603.17104 — specification tracking already exists.
- Provider: https://api-docs.deepseek.com/guides/responses_api/
- Search: https://api-docs.deepseek.com/guides/search/

The research claim is the tested joint effect of revision-aware action eligibility and selective
repair, not a first-ever claim about memory, agents, evidence graphs, or verification.
