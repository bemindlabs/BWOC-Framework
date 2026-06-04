---
title: Design System
parent: English
nav_order: 14
---

# Design System — tokens for BWOC's user interfaces

The single source of truth is **`bwoc-core::design`** (`crates/bwoc-core/src/design.rs`). Three UIs consume it: `bwoc dashboard` (ratatui), `bwoc chat --tui` (ratatui, the `bwoc-tui` crate), and the desktop chat (`bwoc-chat`, egui). Before tokens, each hardcoded its own palette and they drifted — yellow meant three different things on one screen, "muted" text was sometimes near-invisible, and two activity states shared a glyph.

Tokens are **plain data** (no ratatui/egui types) so `bwoc-core` stays dependency-lean and any frontend can consume them.

## Principles

1. **Redundant coding** — a state is never colour-only: every status pairs a *distinct glyph* with a label. The activity glyph set is pairwise distinct, guarded by a unit test.
2. **Signal economy** (Mattaññutā) — zero renders as `—`; attention indicators appear only when non-zero. Surface only what matters.
3. **One meaning per colour per screen** — selection must not reuse the idle/title hue (guarded by a test).
4. **Theme respect** — terminal UIs use a token's `ansi` half (named ANSI colour → the user's terminal theme decides the shade); only pixel UIs (egui) use the `rgb` half.

## Colour tokens (`design::color`)

Each token is a `ColorToken { ansi, rgb }` — pick by **role**, never by hue.

| Token | Role | ANSI | RGB |
|---|---|---|---|
| `ACCENT` | brand/interaction accent — active-pane borders, key labels, counts | Cyan | `53C2D6` |
| `TITLE` | product title / banner heading | Yellow | `E0C060` |
| `SELECTION_BG` / `SELECTION_FG` | selected row (deliberately **not** yellow) | Blue / White | `2D5B9E` / `F5F5F5` |
| `WORKING` | session actively doing work | Green | `9EE093` |
| `IDLE` | live session, no recent output | Yellow | `E0C060` |
| `RUNNING` | process up (distinct from WORKING) | Cyan | `53C2D6` |
| `STALE` | marker present, process gone | Gray | `9A9A9A` |
| `MUTED` | de-emphasised but readable (floors at Gray, never DarkGray) | Gray | `9A9A9A` |
| `SUCCESS` / `WARNING` / `DANGER` | outcomes | Green / Yellow / Red | `9EE093` / `E0C060` / `E09090` |
| `USER` / `SYSTEM` | chat transcript roles | Blue / Gray | `6CB6FF` / `9A9A9A` |

## Glyph tokens (`design::glyph`)

| Token | Glyph | Meaning |
|---|---|---|
| `ACTIVITY_WORKING` | `◉` | actively working |
| `ACTIVITY_IDLE` | `◑` | live, no recent output |
| `ACTIVITY_RUNNING` | `●` | process up |
| `ACTIVITY_STALE` | `○` | marker present, process gone |
| `ACTIVITY_NONE` | `—` | no session |
| `RUNTIME_ALIVE` / `RUNTIME_DEAD` | `●` / `○` | daemon liveness |

## Spacing & typography (`design::space`)

| Token | Value | Meaning |
|---|---|---|
| `MESSAGE_GAP` | `8.0` | vertical gap between transcript messages (egui points) |
| `LINE_HEIGHT_FACTOR` | `1.4` | body line height ÷ font size — room for stacked Thai vowel/tone marks |

## Consuming tokens

**ratatui** — map the `ansi` half to a named colour so terminal themes apply (each TUI carries this ~12-line `tone()` mapper):

```rust
use bwoc_core::design;
fn tone(t: design::ColorToken) -> Color { match t.ansi { Ansi::Cyan => Color::Cyan, /* … */ } }
// e.g.
Style::default().fg(tone(design::color::ACCENT))
```

**egui** — use the `rgb` half directly:

```rust
let (r, g, b) = design::color::USER.rgb;
egui::Color32::from_rgb(r, g, b)
```

Changing the palette is a one-file edit in `design.rs`; the invariant tests (glyphs pairwise distinct, selection ≠ idle/title hue, muted ≠ DarkGray) keep the principles enforced.

## See also

- `crates/bwoc-core/src/design.rs` — the tokens + invariant tests.
- [`HARNESS.en.md`](HARNESS.en.md) — the runtime behind the chat UIs.
