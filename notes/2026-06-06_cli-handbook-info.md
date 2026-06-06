# 2026-06-06 — CLI: `bwoc handbook` + `bwoc info`

Two user-facing self-service commands for `bwoc-cli`. Requested alongside
"auto-check new version on `bwoc`/`bwoc info`" — which turned out to **already
exist** (`update::notify_if_drifted`, run on every invocation with a 24h
throttle cache), so the version-check work was just *surfacing* it via `info`.

## What changed

- **`bwoc handbook [section]`** (`handbook.rs`) — a bundled, offline quick guide.
  Static section table baked into the binary (no network, no file lookup):
  start · agents · spawn · teams · harness · release. `bwoc handbook` prints the
  index; `bwoc handbook <name>` prints one section; unknown → error + list.
  **Bilingual**: resolved language (`--lang`/`BWOC_LANG`/`LANG`) picks the Thai
  body, English fallback.
- **`bwoc info [--json]`** (`info.rs`) — one status card: version
  (`CARGO_PKG_VERSION`) + release identity (`option_env!("BWOC_RELEASE_CALVER")`)
  + phase + workspace + registered-agent count + update-drift line. `--json` for
  scripts.
- **`update::info_status_line()`** — new pub helper that reads the existing
  throttle cache (no network) and returns the drift status for `info`.

## Decisions

- **Handbook content inline (static `&[Section]`), not files.** One module, no
  new asset files, bundled automatically, offline by construction. Terminal-sized
  and task-oriented (Mattaññutā) — full reference stays in `docs/`. Not subject to
  the `docs/en`↔`docs/th` parity audit (it's a bundled CLI asset), but authored
  bilingually anyway since the Thai audience uses the CLI directly.
- **`info` reuses the update cache, never the network.** The background check
  already maintains `~/.bwoc/update-check.json`; `info` reads it. Keeps `info`
  instant and offline-safe; `option_env!` ⇒ source builds show "source build (no
  release identity)" rather than a bogus status.
- **Phase is a `const`** in `info.rs` (manually bumped on phase change; rare).
  Version numbers are authoritative; phase is a coarse label.

## Tests

- `handbook.rs`: every section has EN+TH+title and unique names; `body` picks
  Thai for `th*` else English; unknown→2 / known→0 / index→0.
- `info.rs`: explicit-workspace resolution; `info` runs without a workspace.
- Smoke: `bwoc handbook`, `bwoc handbook teams`, `bwoc info` render correctly
  (info resolved this workspace's 11 agents; source-build status shown). 740
  bwoc-cli tests pass; fmt + clippy clean.

## Status / deferred

- Done. **Next (separate PR):** `bwoc report` → file a GitHub issue from the CLI
  (the third request in this batch).

## Related

- `crates/bwoc-cli/src/{handbook,info,update}.rs`, `main.rs` (Commands + dispatch)
