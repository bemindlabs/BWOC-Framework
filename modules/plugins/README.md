# `modules/plugins/` — Framework Plugins

**Status:** shipped. 28 plugins across 9 kinds, and all of them target **BWOC 3.x**.

A plugin extends the framework with a capability the **workspace** turns on once for everyone. It is not the same as:

- **Agent skills** (`modules/agent-template/skills/`), which an individual agent declares.
- **Framework skills** ([`modules/skills/`](../skills/)), the baseline capabilities any agent can opt into.
- **Declared backends** (`claude`, `antigravity`, `codex`, `kimi`, `copilot`, `grok`, `ollama`, `openai-compatible`, `openrouter`, `litellm`). These are first-class and live in the spec, not in plugins.

The full contract (kinds, manifest, lifecycle, trust gate) is in [`docs/en/PLUGINS.en.md`](../../docs/en/PLUGINS.en.md).

## Installed plugins

| Kind | Count | Plugins |
|---|---|---|
| `audit` | 7 | [`audit-ieee-1012`](audit-ieee-1012/) · [`audit-iso-20000-1`](audit-iso-20000-1/) · [`audit-iso-27001`](audit-iso-27001/) · [`audit-iso-29110`](audit-iso-29110/) · [`audit-iso-9001`](audit-iso-9001/) · [`audit-iso-iec-ieee-12207`](audit-iso-iec-ieee-12207/) · [`audit-iso-iec-ieee-29148`](audit-iso-iec-ieee-29148/) |
| `council` | 1 | [`council-sangha-7`](council/council-sangha-7/) |
| `figma` | 1 | [`figma-rest`](figma/figma-rest/) |
| `gws` | 7 | [`gws-auth`](gws/gws-auth/) · [`gws-calendar`](gws/gws-calendar/) · [`gws-docs`](gws/gws-docs/) · [`gws-drive`](gws/gws-drive/) · [`gws-gmail`](gws/gws-gmail/) · [`gws-sheets`](gws/gws-sheets/) · [`gws-slides`](gws/gws-slides/) |
| `jira` | 1 | [`jira-cloud-rest`](jira-cloud-rest/) |
| `llm-backend` | 2 | [`hermes`](llm-backend/hermes/) · [`openclaw`](llm-backend/openclaw/) |
| `memory-backend` | 1 | [`memory-tier2-noop`](memory-tier2-noop/) |
| `okr` | 1 | [`workspace-okrs`](okr/workspace-okrs/) |
| `workflow` | 7 | [`accounting-api`](workflow/accounting-api/) · [`gcloud-auth`](workflow/gcloud-auth/) · [`gcloud-compute`](workflow/gcloud-compute/) · [`gcloud-iam`](workflow/gcloud-iam/) · [`gcloud-project`](workflow/gcloud-project/) · [`gcloud-run`](workflow/gcloud-run/) · [`gcloud-storage`](workflow/gcloud-storage/) |

## 3.0: `compat` is enforced

Every `manifest.toml` must declare a **bounded** range:

```toml
[plugin]
compat = ">=3.0.0, <4.0.0"
```

- **Mismatch.** A resolver refuses to run the plugin. `bwoc plugin show` marks it `WILL NOT LOAD` rather than hiding it.
- **Unbounded or unparseable range.** `bwoc check` reports a violation.
- **Migrating.** `bwoc migrate` deliberately does **not** rewrite `compat`. Only the plugin's author can say which framework versions it works with.

## CLI

```bash
bwoc plugin list | show <name>
bwoc plugin init <name>                 # scaffold from modules/plugin-template/
bwoc plugin install <path|git|tarball>  # SHA-256 trust gate
bwoc plugin enable|disable <name>       # [plugins.<name>] in workspace.toml
bwoc plugin remove <name>
```

## What plugins are NOT

- **Not a loophole for per-vendor logic in the spec.** Vendor-specific phrasing in `AGENTS.md` is still forbidden (Samānattatā).
- **Not a place for one-off scripts.** Those belong with the agent that uses them.

## See also

- [`modules/README.md`](../README.md)
- [`docs/en/PLUGINS.en.md`](../../docs/en/PLUGINS.en.md) · [`docs/en/COMPATIBILITY.en.md`](../../docs/en/COMPATIBILITY.en.md)
