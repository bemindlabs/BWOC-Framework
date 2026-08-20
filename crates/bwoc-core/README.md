# bwoc-core

Shared types and primitives for the [BWOC framework](../../README.md) — manifest, workspace, trust labeling, and the small helpers every other crate needs.

Eight of the workspace's eleven crates depend on it ([`bwoc-cli`](../bwoc-cli/), [`bwoc-agent`](../bwoc-agent/), [`bwoc-harness`](../bwoc-harness/), [`bwoc-a2a`](../bwoc-a2a/), [`bwoc-connect`](../bwoc-connect/), [`bwoc-mqtt`](../bwoc-mqtt/), [`bwoc-tui`](../bwoc-tui/), [`bwoc-loop-tui`](../bwoc-loop-tui/)) — the exceptions are `bwoc-signing` (a leaf library) and `bwoc-deep-memory`, which is deliberately out-of-process and talks to the framework over a CLI contract, not a type. Because so much links against it, this crate is **dep-quarantined**: the only dependencies are `serde`, `serde_json`, `toml`, and `thiserror`. Nothing async, no HTTP client, no date crate, no TUI types. A module that would need one of those either stays plain data here (the UI palette is `(u8, u8, u8)`, not a `ratatui::Color`) or lives in the crate that owns the dependency. Filesystem I/O is allowed and used — several modules own on-disk formats.

## Scope

- **`manifest`** — `config.manifest.json` for an incarnated agent: `Manifest` (`load_from_path` / `save_to_path`), `TrustBlock`, `TrustDeclared`, `RefusalMode`, `ManifestError`.
- **`workspace`** — `.bwoc/workspace.toml` and `.bwoc/agents.toml`; the agent registry and per-agent `inbox_path`. Spec: [`WORKSPACE.en.md`](../../docs/en/WORKSPACE.en.md).
- **`trust`** — ingress trust labeling. `Principal` (immutable, persisted provenance) and `TrustLevel` (derived, **never** serialized), plus `BackendTrust` for whether a backend's tool execution can be confined.
- **`chat_proto`** — the JSON-line wire format (`ChatInput` on stdin, `ChatEvent` on stdout) between a `bwoc-harness --chat` subprocess and a frontend. Exists precisely because of the dep-quarantine: the CLI drives the harness as a process, not a library.
- **`team`** — Saṅgha membership and the shared task list: `Team`, `Task`, `TeamChatMessage` and their state-transition rules (locking stays at the CLI layer).
- **`routing`** — `.bwoc/interconnect/routes.toml`: `Routes::load` / `resolve`, `RouteKind::{Agent, Namespace}`, `SharedAllowlist`, and `redact_broker` for logging broker URLs without credentials.
- **`inbox`** — the single idempotent writer for `.bwoc/inbox.jsonl`; `append_envelope_deduped` suppresses a re-delivered `messageId`.
- **`outbox`** — sender-side spool at `.bwoc/outbox/<recipientId>.jsonl` for peers that were offline; re-delivery replays the signed envelope verbatim so inbox dedup makes retry effectively-once.
- **`idempotency`** — durable ledger with `seen_or_record` (act once per key) and `latch` (edge-trigger: fire only when a key's recorded value changes, stay quiet while it repeats — the first observation counts as a change).
- **`loop_control`** — `Ticker` (floored cadence) and `Budget` (iteration ceiling). Spec: [`LOOP-ENGINEERING.en.md`](../../docs/en/LOOP-ENGINEERING.en.md).
- **`env_scrub`** — `ENV_ALLOWLIST` + `scrub_env()`, the one credential-free environment handed to less-trusted children (harness sandbox commands, third-party audit plugins).
- **`exec`** — `sibling_binary(name)`: resolve another BWOC binary next to the running executable first, then `CARGO_BIN_EXE_*`, then `$PATH`, so a dev build never launches a stale installed copy.
- **`ipc`** — endpoint naming shared by `bwoc-agent --serve` and its clients; derives the deterministic Windows named-pipe name from the agent directory (Unix uses `<agent>/.bwoc/agent.sock`).
- **`deep_memory`** — the optional Tier 2 seam: the `DeepMemory` trait (`wake_up` / `search` / `mine` / `status`) plus `DisabledDeepMemory` when `deepMemoryCmd` is unset.
- **`doc_kind`** — registry for `notes` / `retrospectives` / `research` and workspace-declared kinds from `.bwoc/doc-kinds.toml`, feeding one generic `bwoc doc` engine. Convention: [`NAMING.en.md`](../../docs/en/NAMING.en.md).
- **`design`** — UI tokens (`Ansi`, `ColorToken`, `glyph`, `space`) shared today by the ratatui frontends (`bwoc dashboard`, `bwoc-tui`). Each `ColorToken` carries both an ANSI name (so a terminal theme keeps authority) and an `rgb` triple for a future pixel UI. Spec: [`DESIGN.en.md`](../../docs/en/DESIGN.en.md).
- **`lifecycle`** — `LifecyclePhase { Uppada, Thiti, Vaya }`, the BWOC arc named per AN 3.47. See [`PHILOSOPHY.en.md` §0.1](../../modules/agent-template/docs/en/PHILOSOPHY.en.md#01-the-arc--uppāda--ṭhiti--vaya).
- **`time`** — `utc_now_iso8601` / `format_iso8601`, hand-rolled so no date crate enters the quarantine.
- **`error`, `identity`** — declared in `lib.rs` but still empty. Errors currently live with their module (`ManifestError`, `WorkspaceError`, `TeamError`, `DeepMemoryError`).

## Usage

In another crate within the workspace:

```toml
[dependencies]
bwoc-core = { workspace = true }
```

```rust
use std::path::Path;
use bwoc_core::manifest::Manifest;
use bwoc_core::trust::{Principal, TrustLevel};

let m = Manifest::load_from_path(Path::new("config.manifest.json"))?;
println!("{} runs on {}", m.agent_id, m.primary_model);

// Trust is recomputed from provenance, never read off disk.
assert_eq!(Principal::LocalOperator.trust(), TrustLevel::Trusted);
assert!(Principal::Tool { name: "read_file".into() }.is_untrusted());
```

## Status

In production use — every BWOC binary builds on it, and the on-disk formats here (manifest, workspace registry, routes, inbox/outbox, team tasks) are the framework's real file contracts. `error` and `identity` remain empty placeholders; the dep list is deliberately frozen at four crates (`proptest` and `tempfile` are dev-only).

## License

[MIT](../../LICENSE).
