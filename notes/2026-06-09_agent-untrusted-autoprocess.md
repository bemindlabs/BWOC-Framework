# 2026-06-09 — Untrusted gateway auto-process

`bwoc-agent --serve` could verify and *announce* a relayed message but never *act* on it — a deployed standalone agent logged arrivals and went silent. This closes the loop: an opted-in agent runs the model on an inbound remote message and replies, so agents on different machines actually converse through the gateway. Part of the approved standalone-agent plan (Component 4, the most security-sensitive: internet input meets a tool-capable harness).

## What changed

- **`crates/bwoc-agent/src/autoprocess.rs`** (new) — `AutoProcessor`. On a *delivered* (Pass/Warn) inbox envelope from a remote (non-`user`) sender, it spawns `bwoc-harness --chat --workdir <agent> [--model/--endpoint/--backend from manifest]`, injects the message as `ChatInput::User` (no explicit principal → `Principal::Unknown` → **`TrustLevel::Untrusted`**), reads `ChatEvent`s (accumulates `Token`, finishes on `Message`), **auto-denies every `PermissionRequest`**, and sends the reply with `bwoc send <from> <reply> --from <self>` (routes back out via the same `transport=gateway`).
- **`crates/bwoc-agent/src/main.rs`** — `serve_core` builds + announces the processor; `check_inbox_for_new` gained an `autoproc` arg and, after the announce/audit, calls `maybe_auto_process` for delivered remote envelopes.

## Decisions / security

- **Untrusted by construction.** The message is fed exactly like a chat-connector turn: omitting the principal defaults it to Unknown→Untrusted, so the harness runs **read-only by default**, capability-denies effectful tools, and jails every tool per turn (Phase 5). stdin is a pipe (non-TTY) so any `ask`-mode tool fails closed; the code also auto-denies permission requests — a remote sender can never approve a tool. This mirrors `bwoc-connect/src/session.rs` (the council's reference).
- **Reply is the agent's own trusted output** — signed with the agent's key via `bwoc send`; the untrusted scope is strictly the inbound turn.
- **Opt-in** via `interconnect/gateway.toml` `auto_process = true` (off by default; resolves `bwoc-harness` + `bwoc` lazily). Reuses the supervised-subprocess + sibling-binary patterns; no new deps in `bwoc-agent` (only `bwoc_core::chat_proto` / `trust::Principal`).

## Status / deferred

- MVP processes one message at a time and **blocks the serve loop** for the turn's duration (acceptable for a dedicated standalone agent; a worker pool / background thread is a follow-up). Replying needs a `routes.toml` `transport=gateway` entry for the peer (deployment config). Long replies go to `bwoc send` as an arg (ARG_MAX bound — fine for bounded turns).
- End-to-end (real model turn → reply across machines) is verified at deploy (Component 5).

## Related

- `crates/bwoc-connect/src/session.rs` — the untrusted-harness-spawn pattern mirrored.
- `crates/bwoc-harness` Phase 5 trust-labeling / capability gate / jail that enforce the read-only posture.
- Plan: `~/.claude/plans/inherited-strolling-porcupine.md`.
