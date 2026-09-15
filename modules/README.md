# `modules/`

These are the framework-level modules. Each subdirectory has its own concern and lifecycle. The Rust runtime lives in [`crates/`](../crates/), and the specification in [`docs/`](../docs/).

| Module | Purpose | Status (3.0) |
|---|---|---|
| [`agent-template/`](agent-template/) | The canonical blueprint `bwoc new` copies into every agent. It is the single source of truth for agent shape and carries **specification 3.0** (`| **Version** | 3.0 |`, checked by `bwoc check`). | Shipped |
| [`plugins/`](plugins/) | Workspace-level extensions, one `kind` each (audit, jira, okr, council, figma, gws, workflow). Each one declares a bounded `compat` range, which is **enforced** in 3.0. | Shipped — 25 plugins |
| [`skills/`](skills/) | Framework skills: the recommended baseline capabilities any agent can opt into. | Shipped — 5 skills |
| [`plugin-template/`](plugin-template/) | The scaffold `bwoc plugin init` copies (`SPEC.md` + `manifest.toml`). | Shipped |
| [`skill-template/`](skill-template/) | The scaffold `bwoc skill init` copies (`SPEC.md` + `manifest.toml`). | Shipped |

## Adding a new module

A new top-level module is a long-term commitment for the framework, so open an issue or RFC before adding one. Each module needs:

- A `README.md` describing its purpose, scope, and status.
- A clear boundary with the modules next to it.
- A statement of how it composes with `agent-template/`, which every agent inherits from.
- If it defines an on-disk format: a `schema_version` marker and a `bwoc migrate` path. See [`COMPATIBILITY.en.md`](../docs/en/COMPATIBILITY.en.md).
