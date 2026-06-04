# 2026-06-04 — TUI design review → P1 fixes → design tokens

One arc in three steps: a UX/design review of `bwoc dashboard` (grounded in the
render code, not screenshots), the four P1 fixes it surfaced (PR #196), and the
structural fix for its P2 findings — a design-token system in
`bwoc-core::design` consumed by both framework TUIs.

## What changed

**Review** (no artifact in-repo; summary in PR #196's description): P1s —
footer legend clipped on narrow terminals; `?` help overlay clipped on short
terminals; no minimum-size guard; `working`/`running` shared the `●` glyph
(colour-only distinction). P2s — yellow meant title+selection+idle at once;
DIM/DarkGray over-use; both body panes equally dim (no focus cue); banner
truncated its meaningful tail. Strengths kept: redundant glyph+label coding,
actionable empty/error states, transient action feedback, `—`/attention signal
economy.

**P1 fixes** (`dashboard.rs`, PR #196): width-responsive footer (<100 cols →
core legend `↑↓ · t · ? · q`); help overlay sized to content + `Wrap`, centred
and clamped; `MIN_COLS×MIN_ROWS` (60×16) guard rendering a "terminal too
small" hint; `working` glyph → `◉`.

**Design tokens** (`bwoc-core/src/design.rs`, this PR):
- `ColorToken { ansi, rgb }` — terminal UIs map the `ansi` half to *named*
  colours (terminal theme keeps authority over shade); pixel UIs (egui) use
  `rgb`. Plain data only — no ratatui/egui types in core (dep-quarantine).
- Semantic colour set (ACCENT/TITLE/SELECTION_*/WORKING/IDLE/RUNNING/STALE/
  MUTED/SUCCESS/WARNING/DANGER/USER/SYSTEM), shared glyphs (activity set +
  runtime pair), spacing (`MESSAGE_GAP`, `LINE_HEIGHT_FACTOR` 1.4).
- Invariant tests encode the principles: activity glyphs pairwise distinct;
  `SELECTION_BG` hue ≠ `IDLE`/`TITLE` hue; `MUTED`/`STALE` never `DarkGray`.
- Both TUIs refactored to consume them via a local ~12-line `tone()` mapper
  (`Ansi` → `ratatui::Color`). P2 resolutions land with the refactor:
  selection yellow → blue/white, agents (navigable) pane gets the ACCENT
  border vs the detail pane's DIM, `DarkGray` → `Gray` for muted/stale.
- Spec: `docs/en/DESIGN.en.md` + `docs/th/DESIGN.th.md`.

## Decisions

- **Tokens live in `bwoc-core` as plain Rust data** (user's choice over a
  `.design/tokens/*.json` + codegen pipeline or a new crate): all three UIs
  already depend on core; JSON would add a build step nobody else consumes
  (Mattaññutā). A JSON export can be generated later if Figma needs it.
- **`ansi` + `rgb` dual encoding.** A single RGB-only token set would force
  TUIs into 24-bit colours and break terminal theming; ANSI-only would leave
  egui with nothing. Both halves live on one token so the semantic choice is
  made exactly once.
- **`tone()` mapper duplicated per TUI (~12 lines)** rather than shared:
  sharing requires a ratatui dep somewhere common — bwoc-core can't take it
  and a helper crate for 12 lines is over-engineering.
- **Status bar in `bwoc chat --tui` is black-on-ACCENT, not SELECTION_*** —
  it's an accent banner, not a selection; first draft mis-assigned it and was
  corrected before commit.
- **Stacked branch.** The token refactor rewrites the same `dashboard.rs`
  regions as PR #196, so `feat/design-tokens` stacks on `fix/dashboard-tui-ux`
  instead of forking main and colliding.

## Status / deferred

- `bwoc-chat` (egui, separate repo) still hardcodes its palette/`LINE_HEIGHT_FACTOR`
  — follow-up PR there to consume `design::color::USER`/`SYSTEM`/accents and
  `design::space::*` via the `rgb` halves.
- P2 "banner truncates the meaningful tail" not yet addressed (truncate the
  path, keep counts/attention) — small follow-up.
- Verified: `cargo test -p bwoc-core design` (4 invariant tests) +
  `-p bwoc-cli dashboard` (3) pass; fmt + clippy (`-D warnings`) clean across
  the three touched crates.

## Related

- PR #196 (P1 fixes), this PR (tokens + TUI consumption)
- `crates/bwoc-core/src/design.rs`, `crates/bwoc-cli/src/dashboard.rs`,
  `crates/bwoc-tui/src/lib.rs`, `docs/en/DESIGN.en.md` (+ TH)
- bwoc-chat PR #2 (egui transcript wrap/line-height — same Thai metrics issue
  the `LINE_HEIGHT_FACTOR` token now records)
