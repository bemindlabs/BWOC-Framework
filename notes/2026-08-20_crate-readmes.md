# 2026-08-20 — A README for every crate, enforced

Repo convention, stated by the architect: **every `crates/<name>/` carries a
`README.md`.** Four crates had one, seven did not — and of the four, three were
actively wrong, describing built-out crates as "Phase 1 v2.0 — scaffold" /
"runtime stub". This lands all eleven, accurate, plus a gate so the convention
holds without anyone remembering it.

## What changed

- **7 new crate READMEs** — `bwoc-harness`, `bwoc-tui`, `bwoc-loop-tui`,
  `bwoc-a2a`, `bwoc-signing`, `bwoc-connect`, `bwoc-mqtt`.
- **4 rewritten** — `bwoc-core` and `bwoc-agent` (both claimed scaffold/stub
  status for crates that are fully built), `bwoc-cli` (missing `loop`,
  `monitor`, `digest`), `bwoc-deep-memory` (fact-checked).
- **`readme = "README.md"` in all 11 `Cargo.toml`s** — previously *no* crate
  declared it, so the file would not ship in `cargo package`.
- **`crates/bwoc-cli/tests/crate_readmes.rs`** — the gate: every crate dir has a
  non-trivial README (≥10 non-blank lines, so a placeholder can't satisfy it)
  and every manifest declares it.

## Why a gate

A convention with nothing enforcing it rots, and this repo has been bitten by
exactly that twice in two days: the root README sat **two releases stale**, and
`bwoc-harness` was missing from **every** release archive because packaging was
never asserted (#460). The framework already applies this pattern deliberately —
`highlights_cite_current_version` fails CI when What's New goes stale. This is
the same shape, ~90 lines, no new CI job (it rides the existing test matrix).

## Written by workflow, then adversarially fact-checked

One agent per crate wrote from the source; a second, high-effort agent
fact-checked each README **against the code and fixed what it found**. That pass
earned its keep — the writers' claims were plausible and several were false:

- **`bwoc-core`**: "Every BWOC binary depends on this crate" — false.
  `bwoc-deep-memory` has *no* `bwoc-core` dependency (verified: 0 hits in its
  `Cargo.toml`); it is deliberately out-of-process, talking over a CLI contract.
  Corrected to "8 of 11".
- **`bwoc-core`**: design tokens "shared by the ratatui and egui frontends" —
  there is no egui anywhere in the workspace.
- **`bwoc-mqtt`**: "`serve` requires `.bwoc/agents.toml`" — false;
  `AgentsRegistry::load` returns `default()` when absent, so serve starts and
  drops every message as `UnknownRecipient` instead. Also mis-attributed
  `BWOC_MQTT_BROKER_FILE` where `send.rs` actually sets `BWOC_MQTT_BROKER`.
- **`bwoc-connect`**: overstated dep-quarantine — only `tokio-tungstenite` is
  exclusive; `reqwest`/`axum`/`rusqlite` live in other crates too.
- **`bwoc-loop-tui`**: claimed CLI writes serialize with a *running loop* via
  `tasks.lock` — false; the harness lead's `JsonlTaskSource` guards with an
  in-process `Mutex` only. The daemon half of the claim is true.
- **`bwoc-harness`**: cited a `fixtures/rename-symbol` eval path that does not
  exist. **`bwoc-signing`**: a stray rustdoc hidden-line marker that would
  render literally on GitHub.

## Verified independently (not just taken from the agents)

- 11/11 crates have a README, 41–63 lines each.
- **Every relative link resolves to a real file on disk** (checked by walking
  each link).
- No YAML frontmatter, no `[[wikilinks]]`, no `> [!callout]` — crate READMEs are
  tier-1 plain Markdown, EN-only (no TH pair required, per `CLAUDE.md`).
- No "scaffold"/"stub"/"Phase 1 v2.0" language remains.
- `cargo metadata` resolves `readme=README.md` for all 11.
- The gate fails when a README is removed and passes when restored (proved by
  temporarily moving `bwoc-mqtt/README.md`).

## Surfaced, deliberately NOT fixed here (one concern per PR)

- `Cargo.toml` `[workspace.package]` has a wrong `repository` URL:
  `https://github.com/bmt-bwol-ops/bwoc-framwork` — wrong org *and* a typo
  ("framwork"). A 2026-06-18 note records fixing this in the README but the
  manifest was missed. Now visible on every crate's package metadata.
- The **root** `README.md` is stale: its Latest-release pointer still says
  `v2026.7.25-4` (2.42.0) against an actual `v2026.8.20-0` (2.44.0), it never
  mentions `bwoc loop` / `monitor` / `digest`, and its crates table omits four
  crates.

## Related

- Convention source: architect instruction, this session.
- Same enforcement pattern: `whats_new::tests::highlights_cite_current_version`.
