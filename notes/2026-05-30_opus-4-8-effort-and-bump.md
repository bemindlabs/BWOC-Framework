# 2026-05-30 — Opus 4.8: reasoning-effort passthrough + model bump

Adapt BWOC to Claude Opus 4.8. Two parts: (1) make Opus 4.8 the default/recommended Claude model across the framework's vendor-allowed surfaces, and (2) add an optional, backend-neutral `reasoningEffort` knob that maps to Opus 4.8's new effort control (and to OpenAI-compatible `reasoning_effort`).

## What changed

**Effort control (new `reasoningEffort` manifest field):**
- **bwoc-core** — optional `reasoning_effort: Option<String>` on `Manifest` (`reasoningEffort` on the wire). Free-form; value space is backend-specific.
- **bwoc-harness** — `OllamaClient` carries an optional effort (`with_reasoning_effort`); `build_request_body` emits `reasoning_effort` only when set. `main` reads it from the workdir manifest and prints an `effort :` line.
- **bwoc-cli** — `bwoc run`'s Claude dispatch appends `--effort <level>` to `claude -p` when the manifest sets it (verified: Claude Code 2.1.158 accepts `--effort low|medium|high|xhigh|max` in headless mode).
- **agent-template** — `reasoningEffort` documented in `config.manifest.json` schema.

**Model bump (Claude → Opus 4.8):**
- `bwoc-cli` spawn catalog (the `bwoc new` picker default) and `bwoc help backends` table → `claude-opus-4-8` first.
- Docs/examples: `docs/{en,th}/INCARNATION` (EN/TH parity), `examples/howto/{first-agent,configure-backends}`.

## Decisions

- **Effort lives in the manifest only — no `{{effortLevel}}` placeholder in AGENTS.md.** Effort is runtime *dispatch* config, not LLM-facing instruction; AGENTS.md carries behaviour, the manifest carries config. Right altitude, and keeps AGENTS.md neutral. *(Mattaññutā + Samānattatā.)*
- **Free-form value, no fixed cross-backend mapping.** Claude accepts `low|medium|high|xhigh|max`; OpenAI-compat accepts `low|medium|high`. Inventing a neutral enum would force a lossy/wrong mapping, so the field passes the operator's literal string and each backend interprets it. *(Yoniso Manasikāra — don't model a mapping we can't verify is correct.)*
- **Passthrough scoped to Claude CLI + harness.** Codex/Kimi/Antigravity effort flags were not verified, so they're left untouched rather than guessed. *(Mattaññutā.)*
- **Verified the Claude `--effort` flag before wiring it** (via claude-code-guide against the installed `claude` binary) instead of assuming it exists.
- **Did NOT bump anti-pattern example IDs** (`neutrality.md`, `conventions.md` show `claude-opus-4-6` as "what not to hardcode"); the version is irrelevant to the point, so churning them adds nothing.
- **`bwoc new` gains no `--reasoning-effort` flag** — operators set it in the manifest by hand, same posture as `autoModels`.

## Alternatives considered

- **Wire effort into the `claude` interactive `spawn` path too** — deferred; `spawn` is the interactive path and the automated `run` path is where per-invocation effort matters most. Can add later if needed.
- **A typed `ReasoningEffort` enum in bwoc-core** — rejected (see free-form decision).

## Status / deferred

- Effort passthrough for Codex/Kimi/Antigravity (pending flag verification per backend).
- `spawn` (interactive) effort passthrough.
- Test fixtures still pin `claude-opus-4-7` (they exercise the mechanism, not model currency) — intentionally left.

## Related (links)

- `crates/bwoc-core/src/manifest.rs` (`reasoning_effort`)
- `crates/bwoc-harness/src/provider/client.rs` (`build_request_body`, `OllamaClient::with_reasoning_effort`)
- `crates/bwoc-cli/src/run.rs` (`build_command` Claude `--effort`)
- `crates/bwoc-cli/src/spawn.rs`, `help.rs` (catalog bump)
- Opus 4.8 announcement: https://www.anthropic.com/news/claude-opus-4-8
