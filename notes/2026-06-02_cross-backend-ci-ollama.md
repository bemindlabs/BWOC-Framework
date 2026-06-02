# 2026-06-02 — Cross-backend validation CI (ollama)

Phase 2's "Cross-backend validation" remaining item had no CI coverage — the
matrix in `ci.yml` only builds + tests on three OSes, never exercising an agent
against a real backend. Added `.github/workflows/cross-backend.yml` to run the
full uppāda → ṭhiti arc on the **ollama** backend (the one needing no API key).
The four vendor backends are a documented follow-up (see below).

## What changed

- New workflow `cross-backend.yml`:
  - **`ollama` job** — installs Ollama, starts `serve`, pulls `qwen2.5:0.5b`,
    builds the bins, then `bwoc init` → `bwoc new --backend ollama` → `bwoc check`
    → `bwoc run --json`, asserting `exit_code == 0` and non-empty output.
  - **Vendor backends** (claude / codex / kimi / antigravity) — a documented
    follow-up, not in this workflow. The first draft included a gated matrix
    using `secrets[matrix.key_env]`; CodeQL flagged it (Excessive Secrets
    Exposure — dynamic indexing forces the whole `secrets` context into scope).
    Dropped it: the user asked for ollama-only, the vendor part is untestable
    without keys, and the clean wiring is one job per backend referencing only
    its own secret — deferred to when an operator provisions the keys.
- Triggers: `push: [main]` + nightly `schedule` + `workflow_dispatch`. **Not**
  `pull_request` — model pulls are slow/network-bound, so this is validation,
  not a required PR gate (ci.yml stays the gate).
- `CHANGELOG.md` `[Unreleased] → Added`.

## Decisions

- **Validated the CLI sequence locally before committing** (a live ollama with
  `gemma4:latest` was available). That surfaced two real bugs in the first draft:
  `bwoc init` requires the target dir to **already exist** (added `mkdir -p`),
  and `bwoc new` needs `--lint-cmd/--format-cmd/--test-cmd/--build-cmd` because it
  cannot prompt for them on a non-TTY (passed `true` as placeholder gates for a
  smoke agent). Yoniso manasikāra — the YAML could not be unit-tested, but the
  command flow it drives could.
- **ollama-only.** The vendor backends need real credentials the repo does not
  hold; rather than block the whole item, ship the ollama proof now. The first
  draft's gated vendor matrix was dropped after CodeQL flagged its dynamic
  `secrets[...]` indexing — the clean form (one job per backend, each scoped to
  its own secret) is a follow-up. Mattaññutā — don't gate a shippable proof on
  credentials we lack, and don't carry untestable scaffolding that trips a
  security rule.

## Status / deferred

- Done locally: the `bwoc init → new → check → run` flow against ollama (exit 0).
- **Unverified in CI:** the workflow YAML itself runs only once pushed to `main`
  (it does not trigger on PRs). First post-merge run may need a follow-up tweak
  (Ollama install path, model warm-up timing). The local dry-run de-risks the
  CLI flow but not the GitHub-runner specifics.
- Deferred: vendor-backend jobs (operator provisions keys; one job per backend
  scoped to its own secret, avoiding the dynamic-indexing CodeQL finding).
