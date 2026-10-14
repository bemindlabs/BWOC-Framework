# 2026-10-14 — Copilot review backlog: bwoc-cli

Triage of the unresolved Copilot review threads left on merged PRs that touch `crates/bwoc-cli`, judged against current `main`. Many were already addressed or wrong (e.g. the "won't compile" claims on `sessions.rs`); the ones that still held are fixed here in one pass, each with a regression test where behaviour changed.

## What changed

- **`bwoc peer learn`** — containment now also resolves symlinks (`resolves_inside`), so a symlink in a shared kind dir cannot leak a non-shared file. Kinds resolve against the **peer's** registry (custom kinds work), and a kind dir that resolves outside the peer workspace is skipped. Listing is built as data (`shared_docs`) so the allowlist test asserts what is listed. `first_heading` streams instead of reading whole files.
- **`bwoc peer view <key>`** — now a real alias of `bwoc peer status <key>` (the help text already advertised it).
- **`bwoc doc new`** — slug is ASCII-only (`[a-z0-9-]+`, matching the note audit); a title with no ASCII letters/digits is a user error instead of producing `YYYY-MM-DD_.md`.
- **`bwoc doc view`** — the name must be a plain stem (no separators / `..`); an ambiguous date prefix lists the candidates instead of picking the first; a directory read error exits 2 instead of "not found".
- **`bwoc spawn`** — an existing but unreadable/invalid `config.manifest.json` is `InvalidManifest`, no longer misreported as `MissingBaseUrl` or silently ignored. The base-url rule is a pure helper, so its test no longer risks launching a real `bwoc-harness`.
- **`bwoc sessions`** — off Unix, markers are treated as alive instead of reaped as stale; marker `pid` uses a checked `u32` conversion.
- **`bwoc fleet health`** — condition 1 reports a missing agent dir instead of counting it as fresh, saturates `--stale-days` math, and says `≥Nd` to match the comparison; condition 5 counts refusals via `livecheck::refusal_summary` (one rule with the dashboard); condition 6 degrades to ℹ when `git user.name` is unset instead of flagging every author.
- **`bwoc chat --team` / team loaders** — `load_team` rejects ids that are not a single path segment.
- **`bwoc outbox flush`** — a spooled line missing `to` / `from` / `messageId` is a hard error, not a delivery with empty ids.
- Docs/comments only: `sessions.rs` module docs, `exit.rs` (`FINDINGS` no longer claims `audit`), `audit run` uses `exit::USAGE`, handbook names `LINE_CHANNEL_SECRET`, `crate_readmes` test requires exactly `readme = "README.md"` in `[package]`.

## Decisions

- Declined: refactoring `run.rs` to pass the loaded manifest into `build_command` (it is validated earlier in `execute()`, so errors are not masked), a `Result` from `fleet::emit_json` (serializing string values cannot fail), PID-keyed fleet fixtures → `tempfile` and renaming the clap `FleetHealthArgs` / `_local_root` (style).
