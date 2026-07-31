# 2026-07-31 — Fleet TUI: @mention routing

Typing `@<agent> <message>` in a fleet pane routes the message to that fleet
member's **live** session — opening its pane and switching to it — instead of
sending to the current agent.

## What changed

- `parse_mention(input)` — pure: pulls a leading `@token` + trimmed remainder;
  `None` for non-mentions (`"email @ me"`, bare `"@"`, empty).
- `Fleet::resolve_agent(name)` — maps a mention to a member index, tolerating the
  `agent-` prefix on either side (`@busaba` ≡ `@agent-busaba`).
- `fleet_send_local` / `fleet_route_to` — extract the deliver-to-a-pane logic:
  the sender's pane notes `→ routed to <target>`, the TUI switches + `open`s the
  target, and delivers via `ChatInput::User`. A bare `@agent` just jumps to the
  pane; a target with no live session (vendor backend) reports the failure
  instead of dropping the message.
- Fleet `Enter` handler now branches on `parse_mention`; a self-mention or an
  unresolved name falls back to a local send (nothing is swallowed).

## Decisions

- **Live pane routing, not inbox** (user's call). `@agent` injects into the
  target's live fleet session — purely a `bwoc-tui` change, no harness / CLI /
  protocol edits, matching the OpenCode-style live fleet. Durable
  `bwoc send`-to-inbox and a hybrid were the alternatives; deferred.
- **Unresolved mention ≠ error.** A leading `@word` that isn't a fleet member is
  sent verbatim to the current pane rather than warned/swallowed — least
  surprising, and lines can legitimately start with `@`.
- **Reused `open` for drivability gating.** Routing to a vendor-CLI agent hits
  the same "no live session" path P3 added, so no new gating logic.

## Status / deferred

- Inline tool-actions (P5) and durable-inbox `@mention` remain.

## Related (links)

- [[2026-07-30_tui-per-agent-manifest]] — the `open`/drivability path reused here.
- [[2026-07-29_tui-fleet-multi-agent]] — the fleet layer.
