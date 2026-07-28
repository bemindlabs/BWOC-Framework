# 2026-07-28 — Human-in-the-loop approval channel (harness half)

PR #1 of the bwoc-mcc "approval console" feature: the Rust side that lets a **non-TTY** fleet agent escalate an `ask`-mode tool to a human out-of-band, instead of the flat fail-safe deny it does today. The macOS control-center UI that answers these requests is a separate follow-up (PR #2, `bwoc-mcc`).

## The gap this closes

`permission::apply_mode`'s `ask` path: TTY → prompt on stdin; **no TTY → fail-safe** (deny for high-blast-radius tools; `default_mode` otherwise). So a `bwoc spawn`-ed fleet agent could never actually *ask* — an `ask` tool was silently denied. The approval channel is the missing "remote TTY".

## What changed

- **`policy/approval.rs`** (new): `ApprovalRequest` (id, agent, tool, **truncated** args preview, trust, ts, timeout) + `ApprovalDecision` (allow, always, by) + `trait ApprovalChannel` + `FileApprovalChannel`. The file channel writes `<workspace>/.bwoc/approvals/pending/<id>.json` **atomically** (tmp + rename, so a watcher never sees a half-write), polls `decided/<id>.json` until `timeout_s`, cleans up both. Same file-queue idiom as the rest of BWOC.
- **`policy/permission.rs`**: `Policy` gains `agent_id: String` + `approval: Option<Arc<dyn ApprovalChannel>>`. The `ask` non-TTY branch, when a channel is present, emits a request and blocks; **`None` (timeout/error) → the exact pre-existing fail-safe**, factored into `fail_safe_ask` so the "no channel" and "channel timeout" paths are provably identical. Default timeout 300s.
- **`main.rs`**: opt-in `--approval-channel` flag attaches a `FileApprovalChannel` at `<workdir>/.bwoc/approvals` + stamps `agent_id`. Off by default.

## Decisions

- **Fail-safe is inviolable.** The channel is an *extension* of `ask`, never a bypass: it can only turn a would-be *deny* into an operator-approved *allow*, never weaken a deny. A dedicated test (`ask_channel_timeout_cannot_weaken_high_blast_radius`) pins that even `computer` + `default_mode=Allow` + channel-timeout still denies.
- **File-based IPC, not a socket/daemon.** Matches BWOC's existing job/scrum file queues, needs no resident process, works over a shared FS (incl. a tailnet mount), and `bwoc-mcc` already watches workspace files. Atomic writes avoid torn reads.
- **Opt-in CLI flag, not a manifest field.** Avoids threading a new field through the 7 `Manifest` literals for a launch-time toggle; the console spawns agents with `--approval-channel`.
- **`always` accepted but one-shot for now.** The decision carries `always`; persisting it as a policy rule is a later slice (logged, honoured for the call).
- **Trust badge deferred.** `ApprovalRequest.trust` exists but is sent empty — threading the turn trust into `apply_mode` is a small follow-up; tool + args + agent already suffice for a human to judge.

## Verification

macOS: fmt + clippy (`--workspace` and `--features test-redteam`) clean with `-D warnings`; workspace tests pass, 0 failed. New unit tests: request preview-truncation + unique ids; file-channel timeout→None and operator-decision round-trip (a thread simulates the console); permission channel allow / deny / timeout-failsafe / approve-high-blast-radius / cannot-weaken. Bemind Linux verification to follow (the channel is OS-agnostic file I/O, but the policy gate is security-critical so it gets the redteam + isolation suites on Linux).

## Status / deferred

- **PR #2 (`bwoc-mcc`, Swift):** `ApprovalWatcher` on the `pending/` dir → notification + popover (Approve / Deny / Always) → write `decided/<id>.json`; menu-bar "pending approvals" section.
- Deferred: `always`-persist as a policy rule; trust badge; a channel that can detect no-listener to short-circuit the wait.

## Related

- `policy/{approval,permission,mod}.rs` · `main.rs` · Phase 5 permission/capability gate · bwoc-mcc (console UI).
