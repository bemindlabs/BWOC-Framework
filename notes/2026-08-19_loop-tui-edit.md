# 2026-08-19 — Loop-Engineering control-center TUI (PR3: in-TUI editing)

Third and final foundational slice of `bwoc loop`. PR1 observed, PR2 drove the
loop; PR3 lets the operator **edit** from the TUI: add a task, adjust the
ticker / budget, and approve or reject a plan-gated task — every task-list write
going through the **locked `bwoc task` CLI path**, never a direct file mutation.

## What changed (`crates/bwoc-loop-tui/src/lib.rs`)

- **`a`** → modal title input (`add_task_input: Option<String>`); `↵` submits
  `bwoc task add`, `Esc` cancels, `Ctrl-C` still quits.
- **`+` / `-`** adjust `ticker_secs` (floored at 1s); **`]` / `[`** adjust
  `budget_iters` (floored at 1). In-memory — applies to the next loop start; the
  Goal pane tags them `(next)` while a loop runs so they don't read as live.
- **`y` / `n`** approve / reject the selected task's plan — offered only when it
  is awaiting review (`requires_plan && plan.is_some() && plan_approved.is_none()`).
  Lead action, no `--as` (the operator is the implicit lead).
- All writes route through `run_bwoc_task(verb, positionals)`: resolves a sibling
  `bwoc`, runs `bwoc task <verb> --workspace <ws> -- <positionals…>` blocking,
  and reloads on success. The lock in the CLI serialises against a running loop.

## Grounding + adversarial review (workflows)

A 3-agent understand workflow mapped the exact `bwoc task` signatures + lock +
plan-gate semantics first. A 3-dimension adversarial review then found and this
fixes (6 confirmed):

- **clap `--` separator (medium)** — user positionals crossed the clap boundary
  unseparated: a `--help` title was a silent no-op (exit 0 → false success), any
  leading-dash title errored, `--workspace=…` was blocked only by accident. Now
  `bwoc task <verb> --workspace <ws> -- <team> <title>` forces every user string
  to a positional. Verified end-to-end: `-- squad "--fix the parser"` adds cleanly.
- **auto-refresh wiped edit errors (medium)** — `reload()` clears `self.error`
  and fires every 2s; an error set outside `reload()` could vanish before being
  drawn. New `set_error` resets `last_refresh` so every message gets a full
  window; all edit-error paths (and the pre-existing guards) route through it.
- **ticker/budget desync while running (medium)** — the live loop captured its
  values at spawn; the pane now tags edited values `(next)` when a loop runs.
- **`is_safe_team_id` allowed a leading dash (low)** — now rejected (flag hazard
  at any exec boundary), independent of the `--` guard.
- **Ctrl-C swallowed in the modal (low)** — now quits even mid-typing.
- **extreme adjust wrapped / budget flip to unbounded (low)** — saturating usize
  arithmetic (no i64 cast); budget floors at 1 so decrementing can't reach the
  `0 = unbounded` sentinel (a semantic reversal). Unbounded stays a launch-flag.

## Decisions

- **No direct `tasks.jsonl` writes** — shelling out to `bwoc task` reuses the
  TaskLock (Sīlasāmaññatā: one writer discipline), so edits are safe under a
  running loop / the daemon. Title/team pass as argv (no shell) → no injection.
- Kept `selected_awaiting_plan` stricter than the CLI (`plan_approved.is_none()`
  only) — the TUI offers approve/reject only for a plan *awaiting a decision*,
  not one already decided; a rejected plan re-enters the offer after resubmit.

## Status

Loop-TUI foundation complete: PR1 observe → PR2 start/stop+log → PR3 edit. Spec
cross-ref (`LOOP-ENGINEERING.{en,th}.md`) + a handbook entry are the remaining
polish, now that the surface is stable.

## Related

- PR2: `notes/2026-08-19_loop-tui-run.md`. Task CLI: `crates/bwoc-cli/src/sangha.rs`.
