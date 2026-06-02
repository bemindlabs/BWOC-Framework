# 2026-06-02 — Cross-backend validation CI (ollama, vendors scaffolded)

Phase 2's "Cross-backend validation" remaining item had no CI coverage — the
matrix in `ci.yml` only builds + tests on three OSes, never exercising an agent
against a real backend. Added `.github/workflows/cross-backend.yml` to run the
full uppāda → ṭhiti arc on the **ollama** backend (the one needing no API key)
and scaffold the four vendor backends behind a secret/variable gate.

## What changed

- New workflow `cross-backend.yml`:
  - **`ollama` job** — installs Ollama, starts `serve`, pulls `qwen2.5:0.5b`,
    builds the bins, then `bwoc init` → `bwoc new --backend ollama` → `bwoc check`
    → `bwoc run --json`, asserting `exit_code == 0` and non-empty output.
  - **`vendor-backends` job** — a `claude / codex / kimi / antigravity` matrix
    running the same flow with `--backend <vendor>` + the vendor key in env.
    Gated `if: vars.RUN_VENDOR_BACKENDS == 'true'`; each step also no-ops with a
    `::warning::` if its `*_API_KEY` secret is absent. Off by default.
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
- **ollama-only in v1.** The vendor backends need real credentials the repo does
  not hold; rather than block the whole item, ship the ollama proof now and leave
  a one-variable switch (`RUN_VENDOR_BACKENDS`) + per-backend secrets to light up
  the rest. Mattaññutā — don't gate a shippable proof on credentials we lack.

## Status / deferred

- Done locally: the `bwoc init → new → check → run` flow against ollama (exit 0).
- **Unverified in CI:** the workflow YAML itself runs only once pushed to `main`
  (it does not trigger on PRs). First post-merge run may need a follow-up tweak
  (Ollama install path, model warm-up timing). The local dry-run de-risks the
  CLI flow but not the GitHub-runner specifics.
- Deferred: vendor-backend activation (operator adds secrets + flips the var).
