---
title: Plugin Template
aliases:
  - plugin-template
tags:
  - group/framework-plugins
  - type/template
  - domain/scaffolding
maturity: template
---

# Plugin Template

> [!abstract] Scaffold for new framework plugins. `bwoc plugin init <name> --kind <kind>` copies this directory to `modules/plugins/<name>/` and substitutes the `{{camelCase}}` placeholders below. The template is **not an installed plugin** — `bwoc check` discovers `modules/plugins/*/manifest.toml` only, so this sibling directory is skipped.

## One Template, Every Kind (Flat Layout)

There is exactly **one** plugin template, not one per `kind`. The plugin author declares the kind via `--kind` on `init`; the value is substituted into `{{pluginKind}}` inside the new manifest. The layout decision (flat vs. per-kind subdirectories) was settled in Sprint 1's path-reconcile:

- Plugins live at `modules/plugins/<name>/`, not `modules/plugins/<kind>/<name>/`.
- `kind` is declared in `manifest.toml` only — never encoded in the path.
- Discovery (`discover_plugin_dirs`) walks one level only, so adding new kinds requires no walker change.

The audit kind landing in EPIC-2 inherits this layout (e.g. `modules/plugins/audit-iso-29110/`, not `modules/plugins/audit/iso-29110/`). See [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md §Rejected]] for the rationale.

## Expected Directory Shape After Substitution

```
modules/plugins/<name>/
├── manifest.toml       # placeholders substituted; `[plugin].kind` reflects --kind
└── SPEC.md             # placeholders substituted; Obsidian-formatted
```

Optional implementation files (Rust crate, binary on `PATH` matching `entry`, config schema attachments, etc.) the plugin author adds after `init` returns. The two required files above mirror the layout declared in [[../../docs/en/PLUGINS.en#manifest|PLUGINS.en.md §Manifest]] — every installed plugin MUST keep them.

## Placeholder Substitution

| Placeholder | Required | Replaced by | Example |
|---|---|---|---|
| `{{pluginName}}` | yes | `bwoc plugin init <name>` argument; kebab-case; must equal the new directory name under `modules/plugins/` | `memory-tier2-noop` |
| `{{pluginKind}}` | yes | `--kind <kind>` flag on `init`; one of `memory-backend`, `llm-backend`, `workflow` (or a future kind added to the enum) | `memory-backend` |
| `{{pluginVersion}}` | yes | Author edit; semver of the plugin itself, separate from the framework version | `0.1.0` |
| `{{pluginDescription}}` | yes | Author edit; one-sentence summary; the **only** manifest value where a vendor name is tolerated | `No-op Tier 2 memory backend that forwards to Tier 1.` |

`compat` is seeded as `">=3.0.0, <4.0.0"` — the framework version range under which this plugin is known to load. Keep the upper bound: an open-ended range would claim compatibility with majors that break this plugin, which is the failure the field exists to prevent. The author tightens or extends the range honestly; a mismatch at load time causes `bwoc` to refuse the plugin (see [[../../docs/en/PLUGINS.en#loading-mechanism|PLUGINS.en.md §Loading Mechanism]]).

`entry` is pre-wired to `bwoc-plugin-{{pluginName}}` — a binary on `PATH` (preferred) or a sibling Rust crate name the framework dispatches to. The same name placeholder is reused so a single substitution wires both the identifier and the entry point.

### `init` vs `install` — Why `--kind` Substitutes Here

`init` and `install` treat `kind` asymmetrically (PLUGINS.en.md §`init` vs `install`):

- **`init <name> --kind <kind>`** — operator declares intent up front; the flag is substituted into `{{pluginKind}}` in this template's manifest. Required because no source manifest exists yet to derive kind from.
- **`install <source>`** — `kind` is read from the source's `manifest.toml`. Not overridable; the source author's declared intent wins.

This template only participates in the `init` path. Sources installed via `bwoc plugin install` arrive already substituted and never touch this template.

## What This Template Is Not

- **Not an installed plugin.** Lives at `modules/plugin-template/` (sibling to `modules/plugins/`), so the plugin-discovery walker — `discover_plugin_dirs` over `modules/plugins/*/manifest.toml` — does not see it. Unresolved `{{placeholder}}` markers therefore never reach `bwoc check`.
- **Not the source of validation rules.** The schema lives in [[../../docs/en/PLUGINS.en#manifest|PLUGINS.en.md §Manifest]]; this file only points to it. If the spec changes, the template follows — never the reverse.
- **Not branched by kind.** A future kind added to the enum (e.g. `audit` in EPIC-2) requires zero template changes — the new kind flows through `{{pluginKind}}` without restructuring.

## Neutrality

Manifest values name no backend, model, or vendor CLI. The `kind` enum values (`memory-backend`, `llm-backend`, `workflow`) are framework-internal categories, not vendor surfaces. The `description` field is the **only** value where a vendor name is tolerated — per the spec's neutrality constraint.

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the spec this template scaffolds against.
- [[../plugins/memory-tier2-noop/SPEC|memory-tier2-noop]] — the first reference plugin; use it as a worked example of a fully substituted manifest + SPEC.
- [[../skill-template/SPEC|skill-template]] — the parallel template for the skill surface.
- [[../../notes/2026-05-26_iso-compliance-plugins|iso-compliance-plugins design note]] — the path-reconcile decision that fixed flat layout for every plugin kind.
