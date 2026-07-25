# 2026-07-25 — `bwoc accounting`: the gated CLI front for the accounting-api plugin

Closes the financial-write gap left open when the `accounting-api` workflow plugin
shipped (2.37.0): the plugin's write verbs (`bill-create`, `bill-update`,
`expense-create`) had no operator boundary — they existed but nothing gated them.
`bwoc accounting` is now that boundary.

## What changed

- New CLI module `crates/bwoc-cli/src/accounting.rs`, modelled on `gcloud.rs` /
  `gws.rs` (own arg structs, own discover/invoke, same 0/1/2/4/255 exit codes).
  Verbs: `report <name>` (READ, free), `bill create`, `bill update <id>`,
  `expense create` (WRITE).
- Wired into `main.rs` (`mod accounting`, `Commands::Accounting`, dispatch).
- `docs/{en,th}/PLUGINS` §Write verbs — added the shared-gate list entries
  (`gws`, `bwoc accounting`) and a new paragraph documenting the **financial-write
  two-part gate**.
- `accounting-api` manifest + `SPEC.{md,th.md}` — dropped the "follow-up slice"
  wording; the CLI ships now.
- Tests: 4 unit (in-module) + 6 e2e (`tests/accounting_cli.rs`).

## Decisions

- **Financial writes get the `gcloud-iam` two-part gate, not the plain `gcloud-compute`
  one.** A purchase doc / expense posts to an external *system of record* and
  auto-posts a double-entry GL entry on the live books — durable and hard to
  reverse. So a per-write confirm alone isn't enough: it also needs a standing
  `[plugins.accounting-api] writes_enabled = true` opt-in (refuse-by-default).
  The opt-in is "these books are writable from here"; the confirm is the per-action
  ack. Reads (`report`) need neither. Grounded in **Appamāda** (heedfulness
  proportional to consequence) — the highest-consequence write class gets the
  strongest gate.
- **The plugin stays gate-free; the CLI is the single choke point.** Same principle
  as every other write-capable plugin (PLUGINS §Write verbs rule 1): one
  confirmation point at the operator boundary, the plugin executes when invoked.
  Invoking the plugin directly bypasses the gate — documented as such.
- **`--params` / `--payload` are validated to a JSON *object* locally** before the
  plugin is ever spawned (rejects arrays/scalars/malformed) — the plugin never
  sees obvious junk (**Yoniso Manasikāra** at the boundary).
- **e2e uses a stub plugin, not the live API.** The real `accounting-api` entry
  hits an HTTP API; the test installs a `workflow/accounting-api` stub that echoes
  a canned envelope, so the discovery → gate → invoke → render wiring is proven
  without network. Gate-refusal cases don't even reach the stub — they assert the
  choke point fires first.

## Alternatives considered

- **Gate inside the plugin (`accounting.sh`).** Rejected — violates the framework's
  gate-at-the-boundary rule and would duplicate the confirm logic per plugin.
- **Per-write confirm only (no standing opt-in).** Rejected — a headless agent
  passing `--yes` (authorized for *one* action) shouldn't be one flag away from
  writing to the books with no workspace-level "this is allowed here" signal.

## Status / deferred

- Shipped 2.38.0 (`v2026.7.25-0`).
- Deferred: `sales` / `cashbook` / `stock` accounting domains (more plugin verbs +
  CLI subcommands); a `report --param k=v` sugar over the raw `--params` JSON.

## Related

- `crates/bwoc-cli/src/accounting.rs`, `tests/accounting_cli.rs`
- `docs/en/PLUGINS.en.md` §Write verbs, `modules/plugins/workflow/accounting-api/`
- Prior: `notes/2026-07-24_accounting-api-plugin.md` (the plugin this fronts)
