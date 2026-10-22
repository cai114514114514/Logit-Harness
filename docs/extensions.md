# Extensions in Logit- Harness

`loh` discovers `SKILL.md` under the project and user directories `~/.loh/skills` and
`~/.agents/skills`. A custom `--home` changes the first user directory. Dependency/build directories
are excluded. Discovery loads YAML `name`/`description` metadata; `skill_load` reads the selected
body or a relative resource. Duplicate names keep distinct IDs and provenance. Changed metadata
requires refreshing the catalog. Symlinked resources and paths escaping the skill directory are
rejected. Executing a skill's scripts still uses the ordinary tool permission checks.

Discovery is lazy: ordinary greetings and knowledge questions do not scan the project for Skills
or load project instructions. Opening slash discovery reads metadata; `/skills` refreshes it.
Use `/<skill-name> literal arguments` or `/skills run <name-or-source-ID> literal arguments`.
Duplicate aliases keep a source choice, and built-in control commands cannot be replaced.
Arguments are user text, including quotes and dollar signs; there is no Shell interpolation.
Browsing the menu does not insert its metadata into model context.

Bundled `/init` creates missing project guidance through normal engineering execution and
independent acceptance. Existing guidance is protected: the workflow proposes changes instead of
overwriting it. In Plan mode it only proposes content. `/review` is read-only; its Git inspection
uses isolated metadata/configuration to avoid executing repository-defined helpers. Filters,
text conversions and submodule/gitlink changes are excluded explicitly; unsupported object/ref
formats fail explicitly. Review those excluded areas separately when needed.

The format follows the [Agent Skills specification](https://agentskills.io/specification).
Skills provide guidance beneath explicit user requirements. Their `allowed-tools` metadata does
not grant new authorization. Project `AGENTS.md` is loaded for the root and the directories of
observed files; `project_instructions` reads rules for a new path. Patches require current rule
hashes. Ordinary shell commands still depend on model compliance with scoped instructions and the
application policy; this is not isolation from arbitrary code.

Markdown cannot register native code, change budgets or grant permissions. A coding workflow
explicitly requested by the user can enter engineering mode, while all workflows stay read-only
inside `/plan` until an explicit user instruction authorizes implementation. No additional plugin
runtime or plugin marketplace is used.

MCP servers are configured in `~/.loh/config.toml`. Both stdio and Streamable HTTP use the official
Rust SDK, currently RMCP 3.6. Its built-in `Auto` lifecycle negotiates `2026-07-28` discovery or
the standard initialize handshake; both paths use the same SDK client and tool admission rules. Tool schemas are loaded on
demand with `mcp_list` and `mcp_schema`; `mcp_call` invokes a discovered tool.

`/mcp` and `/mcp inspect <server>` inspect configuration locally. `/mcp reconnect <server>`,
`/mcp enable <server>` and `/mcp disable <server>` apply after active calls settle. Disabled state
belongs to the session. Capability order remains stable; configuration changes that affect the
cached input prefix are recorded. Neither disabling nor reconnecting silently resolves uncertain
external writes. `/tools` shows capabilities and prerequisites, not a promise that a remote service
is currently reachable or that every concrete action is already authorized.

```toml
[mcp.local.transport]
type = "stdio"
command = "/absolute/path/to/server"
args = []
environment = ["MY_SERVICE_TOKEN"]

[mcp.local.effects]
lookup = "read"
edit = "write"

[mcp.remote.transport]
type = "http"
url = "https://example.com/mcp"
headers = { Authorization = "MY_AUTHORIZATION_HEADER" }

[mcp.remote.effects]
lookup = "read"
publish = "external"
```

Environment/header entries contain variable **names**, never secret values. The HTTP variable holds
the full header value, including any `Bearer ` prefix. HTTP redirects are refused; HTTPS is required
except on loopback. Child servers receive basic process environment plus explicitly named variables.

Effects are declarations by the person configuring the server, not guarantees inferred from server
annotations. Unlisted or unsupported effects require approval. Readonly workers cannot invoke
mutating MCP tools. Local writes share the central source lock and version/hash checks. Cancelling
a local call stops its owned process group and waits for exit; remote writes cannot provide that
guarantee, including remote operations performed by a stdio server. External or unknown effects
remain uncertain independently of transport. An interrupted remote mutation leaves a durable pending record and blocks further writes
to that server. Read tools remain available to inspect reality. `mcp_pending` reports the record;
`mcp_resolve` requires explicit approval of the exact record hash and reconciliation evidence.

Asynchronous MCP task/input continuations are returned as incomplete, never accepted as success.
This version does not automatically drive those optional protocol extensions. A local unfinished
writer is terminated; a remote unfinished writer remains pending for reconciliation.
