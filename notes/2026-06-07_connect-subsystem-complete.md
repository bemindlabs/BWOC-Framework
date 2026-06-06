# 2026-06-07 — Chat-connector subsystem complete

Capstone for the `bwoc-connect` subsystem: chat platforms (Telegram + Discord)
bridged to BWOC agents, DM + group, daemon-supervised. Shipped across 7 PRs
(#218–#224); this note indexes the per-stage notes and records the cross-cutting
decisions + the two CI saves from the final hardening session.

## The PRs

| PR | Stage | Note |
|----|-------|------|
| #217 | design | `2026-06-06_chat-connectors-design.md` |
| #218 | PR1 — Telegram DM + the crate | `…_connect-pr1-telegram-dm.md` |
| #219 | PR2 — Telegram group ⇄ team chat | `…_connect-pr2-telegram-group.md` |
| #220 | PR3 — daemon supervision | `…_connect-pr3-daemon-supervision.md` |
| #221 | PR4 — Discord gateway | `…_connect-pr4-discord.md` |
| #222 | hardening (review follow-ups) | *(this note)* |
| #223 | `bwoc status` connector health | `…_connect-status-health.md` |
| #224 | keyring token resolution | `…_connect-keyring.md` |

## Architecture (the through-line)

- **One transport-agnostic bridge.** `run_bridge` owns all routing — allow-list
  (closed by default), mention-gating, DM-vs-group split, team `chat.jsonl`
  append, session spawn-or-reuse + dead-session respawn. Adding Discord (#221)
  was a new `Transport` impl + a pure `parse_message_create`; the bridge was
  reused **unchanged**. The `Transport`/`AgentSession`/`SessionFactory` seams are
  what made that possible (and keep `run_bridge` unit-testable with mocks).
- **The bridge is just another `chat_proto` frontend** — sessions are
  `bwoc-harness --chat` subprocesses, same protocol as `bwoc chat --tui`. A
  remote user can never approve a tool call: `PermissionRequest` is auto-denied.
- **Dep-quarantine held end to end.** `reqwest`, `tokio-tungstenite`, `keyring`
  live only in `bwoc-connect`; `bwoc-agent` *supervises* the binary as a child
  (the `bwoc-harness` spawn pattern), never depends on it. Verified absent from
  cli/agent/core (`cargo tree -i`).

## #222 hardening (the PR with no standalone note)

Post-PR4 review follow-ups: Discord heartbeat waits ~½ interval (`interval_at`)
instead of firing immediately after IDENTIFY, and `interval_ms.max(1)` so a
malformed HELLO can't panic; `createMessage` failures include Discord's JSON
error body (capped 300 chars); group peer `from` tag is platform-aware
(`GroupBridge.peer_prefix` → `dc:`/`tg:`) instead of hard-coded `tg:`;
`futures-util` `std`.

## Decisions / CI saves

- **keyring: Linux is env-only (Mattaññutā).** Wiring Secret Service pulls
  `dbus-secret-service` → `libdbus-sys` (a **system C lib**; ubuntu CI has no
  `dbus-1.pc` → build fails — caught on the first #224 run), or zbus + a second
  async runtime bridged into tokio. Too much weight/risk for a feature the
  headless deployment target can't use (no Secret Service daemon → env anyway).
  macOS/Windows use the native store; env is the fallback everywhere. "The
  smaller spec beats the more complete one."
- **Token trimmed on resolve** — a keyring/env value with a trailing newline
  (echo/copy-paste) would otherwise corrupt the `Bot <token>` header.
- **Review discipline under autonomy.** The final session ran as a `/loop`;
  Copilot threads were triaged honestly — real findings fixed (heartbeat,
  peer-tag, atomic marker write, untrimmed token), stale ones (lockfile claims
  from a pre-fix commit) resolved with a verification note, never silently.

## Deferred (not omitted)

- **Discord gateway RESUME** — reconnect without a full re-IDENTIFY. YAGNI on the
  one integration-untested edge: fresh-IDENTIFY reconnect already works; RESUME
  is an unverifiable optimization. Build it when real use shows IDENTIFY
  rate-limiting / guild-state-refetch cost.
- Media, slash-commands, multi-group-per-agent — future, if asked.

## Related

- `crates/bwoc-connect/` (lib/telegram/discord/session/main), `crates/bwoc-agent/src/connectors.rs`, `crates/bwoc-cli/src/status.rs`
- the 7 per-stage notes above
