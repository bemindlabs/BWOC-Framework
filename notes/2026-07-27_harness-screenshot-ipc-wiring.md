# 2026-07-27 — Harness multimodal: screenshot IPC wiring (end-to-end)

The last deferred harness follow-up. #385 landed provider-neutral image *serialization* (a `ChatMessage` can carry `ImageBlock`s); this PR wires the one in-harness image producer — the `computer` tool's screenshot — **through the Phase-5 re-exec turn-executor IPC boundary** into that field, so a captured screenshot actually reaches the model. Gated behind `--features browser` (the only build where `computer` is registered + marshallable); default builds are unchanged.

## Why this one genuinely needed bemind

Unlike the MCP HTTP transport (OS-agnostic), this widens the marshalling protocol of the **process-isolation boundary** — the `socketpair` frame a re-exec'd, Landlock/seccomp-jailed child uses to return tool output to the parent. Correct behaviour under the real Linux jail (not just serde round-trips) is the thing to prove, so it was verified on bemind (Ubuntu 24.04, kernel 6.17), not only macOS.

## What changed (the full chain)

1. **`ToolOutput { content, images }`** + defaulted `ToolImpl::execute_rich` (`tools/mod.rs`) — wraps `execute` text-only; only `ComputerTool` overrides it. Every other tool is untouched.
2. **`ComputerTool::execute_rich`** (`tools/computer.rs`) — a `Screenshot` observation becomes an `ImageBlock` via a hand-rolled `base64_encode` (no dep added, dep-quarantine clean); other actions stay text-only.
3. **`dispatch_rich`** (`tools/registry.rs`) — `ToolOutput`-returning sibling of `dispatch` (now a thin `.content` wrapper, so all existing callers/tests are unchanged).
4. **IPC widening** (`turn_executor.rs`) — `WireResponse` gains `#[serde(default)] images` (backward-compatible frame); `run_in_process`/`execute_one`/`respond` carry `ToolOutput`; `ExecutorOutcome`/`ExecOutcome` and `execute_via_isolated_process` propagate `images`.
5. **Agent loop** (`agent_loop/execute.rs`, `mod.rs`) — `ToolCallResult.images` (empty on every denial path) → `ChatMessage::tool_result(...).with_images(...)`.

## Decisions

- **`execute_rich` default method, not a trait-wide signature change.** Adding a defaulted method keeps the blast radius to `ComputerTool` + the child dispatch path; every other `ToolImpl` compiles unchanged. Rejected: turning `execute`'s return into a rich type (touches every tool + test).
- **Hand-rolled base64.** ~25 lines, pure, unit-tested against RFC vectors — cheaper than a dep in a security-quarantined crate.
- **No trust-model change.** A screenshot rides in a `tool_result`, which is already `Principal::Tool` (Untrusted). Images are inert data (base64 string + media type), so nothing new escalates. Frame stays under the 64 MB `MAX_FRAME` cap.
- **Gated behind `browser`.** `computer` is only registered/marshallable with that feature; on default builds `images` is always empty and the paths are inert.

## Verification

- **macOS:** fmt + clippy clean with `-D warnings` on `--workspace`, `--features test-redteam`, and `--features browser`; full workspace tests pass, 0 failed. `tests/process_isolation.rs` 12/12. New unit tests: `execute_rich` screenshot/click, base64 vectors, `WireResponse` image round-trip + imageless backward-compat.
- **bemind (Ubuntu 24.04, kernel 6.17):** _(filled in from the live run — see PR body)_ process-isolation suite + `--features browser` build/tests + redteam clippy under the real Landlock/seccomp jail.

## Status

Completes the "go next all" harness roadmap. Remaining backlog: structured output (audit item 5) stays parked (no in-harness consumer). Live browser-executor screenshots (real Chromium, vs the `MockExecutor`) are exercised only under the `browser` feature's own live tests.

## Related

- #385 (image serialization) · Phase 5 t5 process isolation (`turn_executor.rs`) · `tools/{mod,computer,registry}.rs`.
