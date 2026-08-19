# 2026-08-19 — #410 rec-3: log an undispatched delivered message

Disposition slice for issue #410. The design pass (and #410's own comment)
concluded #412 satisfied the substance — a warm per-sender message pool — and
that rec-1 (unify onto the resident `--headless`) is architecturally **rejected**
on trust-latch grounds (`SessionTrust` is a monotonic boolean latch, so an
Untrusted message would permanently taint a shared trusted-task session). The one
genuine residual was **rec-3**: a delivered remote envelope that the daemon can't
auto-process was announced but then *silently* dropped.

## What changed

- `crates/bwoc-agent/src/main.rs`: after a delivered (Pass/Warn) inbox envelope
  is announced, the `delivered && autoproc.is_active()` branch now has an
  `else if delivered` companion that calls `announce_undispatched(trimmed)` — one
  operator-visible stderr line when auto-process is **off**. It skips
  `user`-origin and malformed/empty lines (same guard as `maybe_auto_process`, so
  only genuine remote messages are hinted), and points at the manual paths:
  enable `auto_process = true` in `interconnect/gateway.toml`, reply by hand, or
  `bwoc task add` (the trigger→task bridge).

## Decisions

- **rec-1 stays rejected, not deferred.** Documented in the #410 close comment so
  it isn't reopened as "just unbuilt". The two disjoint warm families (trusted
  task resident + untrusted per-sender pool) are correct, not a gap.
- **No new dispatcher.** A shared `Dispatch` seam keyed by (trust-tier ×
  conversation) is the real residual, but it earns its place only once a third
  route exists (inbound-service / the L3 middle tier). Tracked in a successor
  issue, deferred (Mattaññutā).

## Related

- Issue #410 (closed satisfied-by-#412 with this rec-3). Partial fix: #412.
- Design proposal: the 6-agent workflow synthesis (#410 + L3 middle tier).
