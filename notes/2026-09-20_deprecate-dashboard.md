# 2026-09-20 — Deprecate `bwoc dashboard` (removal in 4.0)

`bwoc dashboard` is deprecated in 3.4 and goes in 4.0. It is observe-only, and acting on an agent from it goes through tmux. `bwoc fleet` (overview) and `bwoc chat <agent> --tui --fleet` (live sessions in one TUI, no tmux) cover it.

## Decisions
- **Warn, don't rewrite.** There is no one-to-one replacement to canonicalize onto, so unlike the 3.2 aliases the command keeps running its own TUI and only prints the `util::deprecated` stderr line. COMPATIBILITY §Deprecated in 3.4 says so explicitly.
- **Not removed in 3.x**: the compatibility contract allows removal only at a major.
- Docs touched only where they recommend the dashboard (COMPATIBILITY, FLEET-GOVERNANCE, README status row). ROADMAP/DESIGN mentions are history or still true until 4.0.

## Status / deferred
- 4.0: delete `crates/bwoc-cli/src/dashboard.rs` and the `Dashboard` subcommand; check whether `bwoc-core::design` and the tmux launch helpers still have other callers.
