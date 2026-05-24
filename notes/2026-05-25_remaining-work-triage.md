# 2026-05-25 — Remaining-work triage (orchestrator)

Triaged "what's left" against current source + open issues. No crate code written (Oracle's lane). Net: one issue verified done, one confirmed correctly-deferred, one stale tracker retired.

## What changed

- `.claude/loop-roadmap.md` — added a **RETIRED** banner. The doc-refinement cron loop is dead (no active cron — `CronList` empty); its `[ ]` checklist is stale (most items shipped under the later implementation iterations recorded in the same file). Live tracker is now `docs/en/ROADMAP.en.md`. History preserved, checklist deactivated. Anattā.

## Decisions / findings

- **#35 `bwoc fleet health` — verified complete, ready to close.** All 7 Aparihāniya conditions implemented in `crates/bwoc-cli/src/fleet.rs` (1/2/4/5 mechanical; 3/6 git-backed — *exceeds* the v1 informational-only slice; 7 informational). 15 tests green. Surface = top-level `bwoc fleet health` (`main.rs:175,181`); condition-1 threshold = `--stale-days`. **Close blocked by permission classifier** (outward-facing write, no explicit auth) — awaiting user OK.
- **#20 cross-workspace verbs — do NOT rebuild.** `view` (PR #24) + `learn` (PR #26, allowlist-gated via `.bwoc/interconnect/shared.toml`) shipped. The only remaining verb, **give-feedback**, is **deliberately deferred** by maintainer decision (2026-05-24): it needs provable cross-workspace identity = Trust v2 signing, itself deferred under #39 / HV2-4. Issue kept open on purpose. Handing it to agent-pi would push parked work — rejected. (Corrected my own earlier suggestion.)
- **#39 harness-v2** — design epic; the Trust v2 signing decision (#20's blocker) lives here. Needs user direction, not an autonomous handoff.

## Status / deferred

- After #35 closes, framework-side buildable work is either deferred-by-decision (#20 give-feedback, Trust v2) or a design epic awaiting direction (#39). No clean autonomous next task — loop ends pending user signal.

## Related (links)

- `crates/bwoc-cli/src/fleet.rs` — 7-condition impl
- `docs/en/ROADMAP.en.md` — live tracker (replaces loop-roadmap checklist)
- `docs/en/FLEET-GOVERNANCE.en.md` — Aparihāniya-7 spec behind #35
- GH #35 (done), #20 (give-feedback deferred), #39 (harness v2 / Trust v2)
