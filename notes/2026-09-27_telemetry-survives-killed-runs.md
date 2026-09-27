# 2026-09-27 — Telemetry that survives a killed run

Team tianting task **t3**: *"Telemetry must survive a killed run: SIGKILL/panic/OOM/sandbox-kill currently emits no span AND no session-metrics.jsonl line (export_otel_span is only reachable from finish())."* It depended on t1, the OTel GenAI-semconv work, which shipped in 3.12 (#584).

## What changed

- `Telemetry::with_journal(dir)` keeps `<workdir>/.bwoc/telemetry-inflight/<session>.json` holding `{pid, endedEpochSecs, record}`. It is written at start and rewritten after every `record_turn`, atomically (tmp + rename). `finish()` removes it once the real line is appended.
- `telemetry::recover_abandoned(dir, sink)` treats a journal as abandoned when its pid is gone (`kill(pid, 0)`, where EPERM still counts as alive) or when it hasn't checkpointed for 24 h, since a pid can be reused. For each one it appends the record with `harness.end_reason = "abandoned"` and `tasksAttempted ≥ 1`, exports the span, and removes the file. An unreadable journal is renamed from `<session>.json` to `<session>.json.corrupt` so it can't block later runs.
- `export_otel_span(record, end)` takes an **end anchor** instead of always using `now`. Otherwise a recovered session's turn windows would be dated at recovery time. A recovered span gets status Error plus `bwoc.end_reason`.
- `run()` in `main.rs` recovers before journaling the new session, using the same `session-metrics.jsonl` path `finish()` uses.
- HARNESS EN and TH gain §"A run that dies" and a caveat that the report arrives late.

## Decisions

- **Report at the next run, not at death.** Nothing in-process survives SIGKILL or OOM, and a signal handler can't cover them. A journal is the only mechanism that covers every way the parent can die. The cost is a late report, which is documented.
- **`end_reason` is snake_case**, matching the other `HarnessBlock` keys (`turns`, `totals`, `provider`). The rest of the record is camelCase, but this block never was.
- **A turn-executor child killed by the sandbox** was never the gap: the parent survives it and `finish()` runs. The gap was the parent itself dying.
- **Non-unix has no cheap pid check,** so only the 24 h staleness rule applies there.

## Verification

- 5 unit tests: the journal follows the session and is removed by finish; a dead pid is recovered once with its turns intact; a live session is left alone unless stale; a corrupt journal is set aside; there's no `end_reason` on a normal record.
- **Live:** a real `bwoc-harness --task` run against a hanging endpoint, then `kill -9` it. That left **0 metrics lines** and one journal (the bug, reproduced). The next run printed `recorded 1 abandoned session(s)` and wrote the line with `end_reason: "abandoned"`, `tasksAttempted 1 / completed 0`, and removed the journal.
- clippy is clean with and without `--features otel`, and harness tests pass (645).
- Not verified: the span actually arriving at an OTLP collector. No collector was running here. The recovery path calls the same exporter as `finish()`, just with an earlier end anchor.
