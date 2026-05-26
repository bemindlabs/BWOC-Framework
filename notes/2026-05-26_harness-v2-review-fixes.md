# 2026-05-26 — Harness v2 review-pass fixes

Code-reviewed the seven `feature/harness-v2` workstreams sitting in `[review]`
(BWOC-2..9) and fixed the correctness/safety defects the review surfaced.
Blockers first (concurrent-tool seam, Saṅgha runtime, retrospective coverage,
otel test coupling), then the lower-severity concerns. Default and
`--all-features` test suites green throughout; no behavioural change to the
v1 safety pipeline beyond making it hold under the new concurrent path.

## What changed

- **`agent_loop.rs` — `execute_tool_calls` split into two phases.** Guardrails →
  permission now runs *sequentially* (decisions collected in input order);
  sandbox → dispatch then runs *concurrently* over the approved calls. Doc
  comment corrected (it claimed "sequentially" while the body was `join_all`).
- **`mcp.rs` — `StdioTransport` gained a `req_lock`.** A whole request/response
  cycle (write → read-until-our-id) is now serialised per transport.
- **`worker.rs` — `SubprocessRunner` hardened.** `kill_on_drop(true)`, a
  `DEFAULT_WORKER_TIMEOUT` (1800s, overridable via `with_timeout`), spawn →
  timed `wait` → kill+reap on elapse. `git_worktree_add` self-heals (prune +
  force-remove a leftover at the path before `add`). New timeout regression
  test (`subprocess_runner_kills_on_timeout`, unix-gated).
- **`queue.rs` — `submit` TOCTOU + leak fixed; cancellation races the worker.**
  Capacity check + in-flight insert under one lock; slot released on send
  failure; `run_worker` selects the worker future against `cancel`. Dead
  `poll_sangha` (+ its 3 tests) removed.
- **`main.rs` — retrospective + metrics now run on every outcome.** `run_loop`
  result is no longer `?`-propagated before the run-end work; `tasks_completed`
  increments only on success so an aborted run surfaces a sub-100% rate.
- **`telemetry.rs` — otel export decoupled from the local append.**
  `export_otel_span` early-returns when no Tokio reactor is in context (the
  JSONL append in `finish` has already happened); `provider.shutdown()` added
  after `force_flush`.

Lower-severity concerns, same session:

- **`checkpoint.rs` / `agent_loop.rs` — resume worktree guard.** `RunState`'s
  dead `telemetry_cursor` field replaced by a canonical `workdir` (`serde(default)`
  for legacy checkpoints); `run_loop` refuses a resume whose `--workdir` doesn't
  match the checkpoint's worktree. New test `resume_workdir_mismatch_is_refused`.
- **`agent_loop.rs` — budget no-usage warning.** One-time warning when a budget
  is configured but the provider returns no usage (the gate would otherwise
  silently never trip).
- **`mcp.rs` — per-request timeout.** `REQUEST_TIMEOUT` (60s) bounds the
  read-until-our-id loop so a hung/misbehaving server can't block forever.

## Decisions

- **Sequential approval, concurrent execution** (not "serialise everything" or
  "spawn each on its own task"). The expensive step (dispatch / `run_command`)
  keeps the HV2-7 parallelism; only the cheap decision step — which may block on
  an operator prompt — is serialised, so an `ask`-mode approval can't be
  misattributed across interleaved prompts. *Sīla — the approval gate must mean
  what it says before parallelism is allowed near it.*
- **MCP: one in-flight request per transport, not a pending-by-id demux task.**
  The line protocol has no response routing; a single reader future that
  discards non-matching ids will steal a concurrent request's reply. A full
  multiplexer (single reader task + pending-by-id map) is the "correct" answer
  but adds a background task + lifecycle to every server connection. A cycle
  lock is the *mattaññutā* fix: correct, ~10 lines, and different servers still
  run in parallel (separate transports → separate locks).
- **Worker timeout defaulted on (1800s), not opt-in.** The lead drain is serial
  today, so one hung worker blocks everything; an unbounded default is the
  unsafe choice. `with_exe` (test stubs) defaults to no timeout to keep the
  instant-binary tests honest.
- **Retrospective runs on failure too.** A budget/iteration/exhaustion abort is
  exactly the §8b signal worth surfacing; skipping it (the old `?`) hid the most
  informative runs. Counting `attempted` always and `completed` only on success
  also revives `LowCompletionRate`, which was dead under the hardcoded 1/1.
- **otel: skip-when-no-reactor, don't make `finish` async.** Keeping `finish`
  synchronous preserves every (sync) call site; the network export is the only
  reactor-dependent part and is best-effort by contract, so a guarded skip is
  the least-invasive correct fix.

## Alternatives considered

- Spawning each tool call on its own `tokio::task` (rejected: needs `'static` +
  `Send` plumbing and reintroduces the panic-swallow the current `join_all`
  avoids; the approval race is solved more cheaply by phase-splitting).
- MCP multiplexer task (deferred: heavier than the defect warrants; revisit if
  request pipelining per server is ever wanted).
- Making `Telemetry::finish` async / `tokio::spawn`-ing the export (rejected:
  churns every call site for a best-effort side-channel).

## Bugs surfaced and fixed

1. `ask`-mode approval gate: blocking stdin under `join_all` stalled the
   executor and let concurrent prompts misattribute approvals. **(safety)**
2. MCP concurrent same-server calls corrupted each other's responses /
   hung. **(correctness)**
3. Failed-worker worktree stranded the task forever (re-claim `add` collided).
4. No worker timeout/kill → hang blocks the lead; dropped future orphans child.
5. `submit` capacity TOCTOU over-admits; in-flight slot leaked on send failure.
6. Queue cancellation couldn't interrupt an in-flight worker.
7. Run-end retrospective never ran on aborted runs; `LowCompletionRate` dead.
8. `telemetry::finish` panicked ("no reactor") under `--features otel` when
   called from a sync context (incl. the unit tests).

## Status / deferred

- All eight blockers **and** the three lower-severity concerns above:
  **fixed + verified**. `bwoc-harness` clippy clean and `cargo test` green on
  both the default and `--all-features` builds (234 tests); workspace builds.
- Not touched: scrum-board status transitions for BWOC-2..9 (still `[review]`);
  merge of `feature/harness-v2`; the stale OS-sandbox row in `HARNESS.en.md`
  §Not-Yet (flagged in the planning note, separate from #39).

## Related (links)

- [`2026-05-25_harness-v2-planning.md`](2026-05-25_harness-v2-planning.md) — the
  epic plan + invariants these fixes were checked against.
- Per-workstream build notes: `2026-05-26_hv2-{1..7}-*.md`,
  `2026-05-26_otel-real-exporter.md`.
- GH #39 (epic), #20 (unblocked by HV2-4, still in progress).
