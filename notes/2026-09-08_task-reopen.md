# 2026-09-08 — `bwoc task reopen`

The first `bwoc-harness --lead --loop` run against a real task list produced a
**false completion**, and the task state machine had no way to withdraw it. This
adds one.

## What happened

Three council-derived tasks were added to team `tianting` and the goal loop was
fired with a two-iteration budget. Two of them carried `--requires-plan`, and
the loop did exactly what it should: claimed them, ran a worker, and stopped at
the Pavāraṇā gate — `blocked: some await plan approval (HELD — needs operator)`.

The third, `t1` ("merge feat/otel-genai-semconv to main"), had **no gate**,
because it looked like a human-only action and I did not think a gate was needed
for something an agent obviously could not do. A worker claimed it and marked it
completed. `git merge-base --is-ancestor 692d237 main` says NOT MERGED; the
branch has never been pushed. The completion unblocked `t3`, which was then
claimed on a premise that was never true.

`bwoc task` had add / list / claim / complete / plan / approve / reject — every
transition moving one way. The only way to correct the record was to hand-edit
`tasks.jsonl`, outside the locked path `CLAUDE.md` requires every write to take,
leaving no trace of what was corrected or why.

## What changed

- **`bwoc_core::team::reopen_task`** — `Completed` → `Pending`, clearing
  `claimed_by`, `completed_at`, and (on a plan-gated task) `plan_approved`.
- **`bwoc_core::team::dependents_of`** — the tasks a reopen was unblocking, for
  reporting.
- **`bwoc task reopen <team> <task> [--reason …] [--json]`** — an operator verb,
  same lock and same load → mutate → save as every other write.
- Documented in `LOOP-ENGINEERING.{en,th}.md` §Withdrawing a completion,
  including the lesson that produced it.

## Decisions

- **Operator action, no `--as`, no membership check.** Every other task verb is
  an agent acting inside the team. This one exists precisely because the fleet's
  own record is wrong, so gating it on fleet membership would be asking the
  party that made the false claim to authorise its withdrawal.
- **The plan verdict is cleared; the plan text is kept.** A surviving
  `Some(true)` would let the next claimant complete without a fresh plan — the
  exact gate a false completion evades. The plan *text* is the claimant's own
  account, useful to whoever picks the task up; deleting it would discard
  evidence rather than correct a claim.
- **Dependents are reported, never cascaded.** An unclaimed dependent re-blocks
  on its own, because `claim_task` re-checks dependency state at claim time. One
  already `InProgress` or `Completed` is a judgment the operator has to make —
  cascading would withdraw assertions this call was not asked to touch.
- **Reopening twice is an error, not a no-op.** A second call means the caller's
  model of the list is wrong, and a silent success would hide that.
- **`--reason` is optional but recorded.** A correction with no stated reason is
  a second unexplained state change.

## Alternatives considered

- **Hand-edit `tasks.jsonl`.** Fastest, and the reason this command exists: it
  leaves the correction outside the locked path and outside any record.
- **Append a superseding task.** Honest to an append-only audit trail, but it
  leaves the false `completed` in place — every later reader still has to know
  the story to distrust it, and `claim_task` would still treat the dependency as
  satisfied.

## Status / deferred

`t1` is reopened with its reason recorded. `t2` and `t3` remain `in_progress`
with plans awaiting the lead, and `t3`'s premise (that the branch is merged) is
still false — the operator decides whether to reopen it too. The command reports
dependents rather than deciding that.

Two findings from the same run that this change does NOT address:

- **The loop's workers cannot do real work with the backends available here.**
  `+0/-0` on both tasks. The vendor-CLI backend is chat-only (no tool calls),
  there is no `ANTHROPIC_API_KEY`, so workers fall back to local Ollama —
  `gemma3-tools:27b` emitted a tool call as literal text rather than invoking
  one, and its plan proposed an `atexit` handler to catch `SIGKILL`, which is
  not catchable. The fleet *votes* on Opus via the `claude` CLI and *works* on a
  27B local model; that gap is worth naming.
- **`export_otel_span` is only reachable from `finish()`** (verified:
  `telemetry.rs:315`, called once at `main.rs:740`), so a killed, panicking or
  sandbox-contained run emits no span **and** no `session-metrics.jsonl` line at
  all. Surfaced by agent-zhongkui in council D2 and confirmed against the
  source. Tracked as `t3`.

## Related

- Council D2 (`~/workspaces/bwoc/.bwoc/council/`) — no concord, 5 acp-adapter /
  3 otel-deepen, where the telemetry defect was named.
- `docs/en/LOOP-ENGINEERING.en.md` §Withdrawing a completion.
