# 2026-08-06 — Inbox tmux wakeup: session resolution + literal send

Ran the live smoke test that [[2026-05-23_inbox-wakeup-and-auto-reply]] deferred
("needs operator hands"), against a real Claude Code TUI on bemind. The
send-keys mechanism itself is **fine on current Claude Code** — the bug was in
how `notify_tmux` *finds* the target session.

## What the smoke test showed

- **Idle Claude:** the exact two-step (`send-keys -- "<text>"` → 200ms → `send-keys Enter`) submits immediately; Claude replied and the input box cleared.
- **Busy Claude (mid-turn):** the same wake is **queued** ("Press up to edit queued messages") and runs when the current turn ends. No loss.
- So the incantation is not the problem. The failure ("ปลุก agents ไม่ตื่น") is that the wake was often sent to a **session name that didn't exist**, silently no-op'ing.

## Root cause

`notify_tmux` targeted **only** the bare `<x>` session (strip `agent-` from `agent-<x>`). But `bwoc chat --tmux` creates its session as `bwoc-<agent_id>` = **`bwoc-agent-<x>`** (`chat.rs`, `new-session -A -s bwoc-<id>`). `has-session -t <x>` misses it → no wake → the agent never sees the message. Manually-wrapped sessions (bare `<x>`, per the earlier note's line-52 workaround) happened to match, which is why it worked sometimes and not others.

## What changed (`crates/bwoc-cli/src/send.rs`)

- `tmux_session_candidates(to)` / `resolve_tmux_session(to)` — try `<x>`, `agent-<x>`, `bwoc-agent-<x>`, `bwoc-<x>` and wake the first live one. Fixes the bwoc-launched-session miss.
- Send the body with **`send-keys -l`** (literal) so a message containing a tmux key token (`Enter`, `C-c`, `;`) is injected verbatim, not reinterpreted as a keypress — a latent correctness bug.
- Doc comment updated with the verified idle/busy behavior.
- Unit test `tmux_candidates_cover_the_launch_conventions`.

## Decisions / not-done

- **Did not touch the two-step / 200ms** — verified working; changing it would risk what already works.
- **Busy-agent latency** (a wake queues behind a long turn) is inherent to Claude Code's one-turn-at-a-time model, not a wake bug — left as-is.
- **Headless / served agents have no tmux → no wake at all** is the separate architectural gap tracked in **#410** (no dispatcher feeds the resident `--headless`); out of scope here.

## Related

- [[2026-05-23_inbox-wakeup-and-auto-reply]] — the original (unverified) mechanism.
- Issue #410 — headless message-ingress gap.
