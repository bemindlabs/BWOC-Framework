# 2026-05-30 — Canonicalize the declared-backend count at SIX

The repo described its declared backends inconsistently — `four` (the vendor CLIs: claude/agy/codex/kimi), `five` (+ ollama), and, after the Opus 4.8 / GPT-5.5 surface refresh (#121), `six` (+ openai-compatible) in some places. The code is unambiguous: `Backend` has six variants and `bwoc new --backend` accepts six values (`claude`, `antigravity`, `codex`, `kimi`, `ollama`, `openai-compatible`). This change makes the docs and the neutrality checker agree with the code at **six**.

## What changed

- **`crates/bwoc-cli/src/check.rs`** — `BACKEND_NAMES` (the neutrality-checker's backend-name denylist) gains `"openai-compatible"`; comment updated to "six". `contains_word` already matches a hyphenated needle (hyphen is a word boundary), so no matcher change. No installed skill/plugin manifest contains the string, so nothing newly fails `bwoc check`.
- **`crates/bwoc-cli/src/help.rs`** — `backends` topic summary "5 declared backends" → "6".
- **`docs/en/PLUGINS.en.md` + `docs/th/PLUGINS.th.md`** — four "five declared backends" statements → six; the two enumerated lists add `openai-compatible`.
- **`docs/en/ROADMAP.en.md` + `docs/th/ROADMAP.th.md`** — the backend-neutrality count "five" → "six".
- **`modules/agent-template/docs/en/SRS.en.md` + `.th`** — Sammā-ājīva row "Four backends" → "Six".
- **`modules/plugins/README.md`** — "four declared backends (Claude, Antigravity, Codex, Kimi)" → six + Ollama, OpenAI-compatible.
- **`examples/howto/configure-backends.md`** — "All four backend CLIs … (CLAUDE/AGY/CODEX/KIMI)" → "All six backends …" + `OLLAMA.md` / `OPENAI.md`.
- **`modules/agent-template/AGENTS.md`** — "all four backends" → "all six backends" (still backend-neutral; re-ran the template neutrality check).
- **`docs/en/ARCHITECTURE.en.md` + `.th`** — backend-symlink enumeration adds `OPENAI.md`.

## Decisions

- **Count = SIX (architect's ruling).** `openai-compatible` is a distinct declared backend, matching the `--backend` enum, even though it shares the `bwoc-harness` execution path with `ollama` (the two differ only by endpoint). *(Yoniso — aligned docs to the code reality rather than the stale prose.)*
- **Left narrative / reality-specific statements at "five".** ROADMAP's "ollama was added as the fifth declared backend" and "5 backend CLIs in CI" are historically/operationally accurate (ollama *was* the fifth; CI exercises the harness once, not openai-compatible separately). Only the *general declared-count* statements moved to six. *(Mattaññutā — don't rewrite true history to satisfy a count.)*
- **`ARCHITECTURE.md` has no numeric count to change** — it describes two execution paths (vendor CLI / `bwoc-harness` serving ollama + OpenAI-compatible), so only its symlink enumeration needed `OPENAI.md`. The "five declared backends from ARCHITECTURE.en.md" citation in `check.rs` was a slight over-claim; corrected to six.
- **Left `CHANGELOG.md` (historical entry) and `examples/usecases/README.md`** ("four backends side-by-side: claude/agy/codex/kimi") untouched — the latter describes a *specific* planned 4-way comparison, not a count of declared backends.

## Related (links)

- `crates/bwoc-cli/src/check.rs` (`BACKEND_NAMES`)
- `crates/bwoc-cli/src/spawn.rs` (`Backend` enum — the source of truth at six)
- `docs/en/ARCHITECTURE.en.md`
