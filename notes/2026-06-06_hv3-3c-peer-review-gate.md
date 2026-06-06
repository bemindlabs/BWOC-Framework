# 2026-06-06 — HV3-3c: Peer-review gate

Final piece of HV3-3 (Saṅgha collaboration; plan:
`notes/2026-06-05_harness-v3-plan.md`). A lead can route a successful worker's
diff to a designated reviewer agent before completing the task — closing the
"a peer reviews the diff before gates" clause of the v3 Definition of Done.

## Decision (architect, 2026-06-06)

**Fixed reviewer per team** (over round-robin / manifest-declared) — smallest
that works, extensible later. Lives as a `reviewer` field on the team.

## What changed

- **`bwoc-core::team::Team.reviewer: Option<String>`** — optional, omitted from
  toml when unset; by convention a team member, never reviews its own work.
- **New `bwoc-harness::review`** — `Reviewer` trait (injectable), `ReviewSpec`,
  `ReviewVerdict { approved, feedback }`, a pure `parse_verdict`, and
  `SubprocessReviewer`: spawns a `bwoc-harness` in the **worker's worktree** (so
  it can `git diff HEAD` + read changed files) with a review prompt, then reads
  the verdict from the HV3-3b result envelope the reviewer leaves behind. Also
  `AlwaysApprove` (the "gate open" reviewer for the no-reviewer path / tests).
- **Lead** (`run_lead` gains a `reviewer: Arc<dyn Reviewer>` param): on worker
  success, if `cfg.reviewer` is `Some` and ≠ `agent_id`, gate on the verdict —
  APPROVE → complete + tear down; REJECT → unclaim (re-queue), keep the worktree
  + log feedback, count in the new `LeadSummary.rejected`. `--reviewer <agent>`
  on lead mode populates it.

## Decisions / rationale

- **Verdict via the HV3-3b envelope, not a new tool/channel.** The reviewer is
  just a harness run; it already writes `.bwoc/worker-result.json`. The reviewer
  is told to make its **first** line `VERDICT: APPROVE` / `VERDICT: REJECT:
  <reason>` so it survives the envelope summary's leading-chars truncation.
  `parse_verdict` is pure + unit-tested; the spawn mirrors `SubprocessRunner`.
- **Fail-safe to REJECT** (Sīla): spawn failure, timeout, or no parseable
  verdict ⇒ rejection, so unreviewed work is never auto-completed. A rejection
  is re-queued (not failed) and the worktree is kept for the next claimer.
- **Self-review skipped**: `reviewer == agent_id` ⇒ no gate (can't rubber-stamp
  your own diff).
- **Injectable `Reviewer`** so the lead loop is tested with a scripted verdict
  (approve completes + cleans; reject re-queues + keeps worktree; self-review
  skipped) without real subprocesses.

## Tests

- `team.rs`: `reviewer` optional + toml roundtrip (absent stays out of the doc).
- `review.rs`: `parse_verdict` (approve / reject+reason / bare reject /
  missing→fail-safe / leading-prose), `AlwaysApprove`, and a cfg(unix) exec test
  (stub binary leaves no envelope → fail-safe reject).
- `lead.rs`: approve→complete+teardown; reject→re-queue+worktree-kept+`rejected`;
  self-review skipped. Existing lead tests updated for the new `run_lead` arg +
  `LeadSummary.rejected`.

## Status / deferred

- HV3-3c done → **HV3-3 complete** (a+b+c). The harness side takes
  `--reviewer <agent>`; **deferred slice:** CLI/team tooling that reads the
  team's `reviewer` field and passes `--reviewer` automatically (mirrors the
  `bwoc chat --team` → `--team-chat` wiring).
- Also deferred: running the reviewer in a read-only/plan mode (today the prompt
  says "do not modify", but it isn't enforced); richer feedback attachment
  (durably onto the task or the team chat log) beyond the current log line.
- Remaining v3: HV3-4 (self-improvement v2, policy+prompt), HV3-5 (live remote,
  MCP-first), HV3-6/7.

## Related

- `crates/bwoc-core/src/team.rs`, `crates/bwoc-harness/src/{review,lead}.rs`,
  `main.rs::run_lead_mode`
- `notes/2026-06-05_hv3-3b-worker-result-envelope.md` (the envelope this rides on)
