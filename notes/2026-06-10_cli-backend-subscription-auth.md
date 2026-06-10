# 2026-06-10 — `cli` backend: subscription-auth vendor CLI as a provider (#277)

MVP of the CLI-backed agent session requested in #277: orgs that block API keys but hold
Claude/Codex subscriptions can now run bwoc agents on those models with **no key** — the
harness drives the locally-authenticated vendor CLI per turn.

## What changed

- **`crates/bwoc-harness/src/provider/cli.rs` (new)** — `CliClient: ProviderClient`. Each
  `complete()`/`stream()` spawns `<cli-cmd> -p --model <model> --output-format json`, pipes
  the **flattened conversation on stdin** (`System:`/`Human:`/`Assistant:` turns; no
  `ARG_MAX` limit), parses the print-mode JSON envelope (`{"result", "is_error"}`), and
  falls back to raw stdout for CLIs without a JSON print mode. 600 s hard turn timeout
  (a wedged CLI must not hang a chat turn). `stream()` emits the finished reply as one
  chunk — print-mode CLIs have no token deltas. `validate_model` checks the binary is
  executable; unknown-model wording from the CLI maps to `ModelNotFound` so the fallback
  chain works.
- **`main.rs`** — `--cli-cmd` flag (default `claude`); `build_provider` gains the `"cli"`
  arm. `ensure_backend_credentials` already no-ops for vendor CLIs.
- **`bwoc-core` `Manifest`** — optional `cliCmd` field; `backend` doc lists `"cli"`.
- **`bwoc-connect`** — `HarnessSessionFactory` now reads `backend` + `cliCmd` from the
  manifest and forwards `--backend`/`--cli-cmd` to the spawned harness. This also fixes a
  pre-existing gap: connect **never forwarded the backend at all**, so non-default-backend
  agents silently ran on Ollama.

## Decisions

- **Chat-only, tools ignored (not an error).** The chat session always passes its tool
  registry; a vendor CLI executes its *own* tools internally and print-mode output carries
  no `tool_calls`. Erroring would break `--chat` entirely; ignoring renders the backend a
  plain conversational model, which is the #277 use case (connect bridges). Documented in
  the module header; agentic tool use stays on HTTP backends.
- **Stateless transcript over `--resume`.** Flattening the harness history each turn keeps
  one source of truth (no CLI-session divergence, works after compaction). The cheaper
  incremental `--resume` path is an explicit follow-up, not MVP.
- **Assistant replies via `ChatMessage::assistant()`** — the constructor stamps the trust
  `Principal`; struct literals are deliberately impossible (private field).
- `bwoc spawn`/`bwoc run` integration (a `Backend::Cli` variant in `bwoc-cli`) is **out of
  scope** for this PR — chat/connect/TUI paths take the backend as a string and work now;
  the spawn enum is a separate concern.

## Verification

- 10 new unit tests (fake-CLI shell scripts, unix-gated): JSON envelope, raw-stdout
  fallback, `is_error` → Provider error, non-zero exit surfaces stderr, unknown-model →
  `ModelNotFound`, missing binary, single-chunk stream, `validate_model`, flatten shape.
- Workspace: 32 suites green, `clippy -D warnings` clean, `cargo fmt` clean.
- **Live against the real `claude` CLI** (subscription auth, no key in env): print-mode
  envelope parsed exactly as implemented (`is_error: false`, `result` extracted).

## Related

- Closes #277 (MVP scope). Follow-ups recorded there: `--resume` continuity,
  `bwoc spawn`/`run` enum integration, optional stream-json token deltas.
