//! Design tokens — the single source of truth for the colours, glyphs, and
//! spacing BWOC's user interfaces render with.
//!
//! Three UIs consume these tokens: `bwoc dashboard` (ratatui), `bwoc chat
//! --tui` (ratatui, the `bwoc-tui` crate), and the desktop chat (`bwoc-chat`,
//! egui). Before this module each hardcoded its own palette, which drifted:
//! yellow meant three different things in one screen, "muted" was sometimes
//! near-invisible `DarkGray`, and two activity states shared a glyph.
//!
//! ## Shape
//!
//! Tokens are **plain data** — no ratatui/egui types — so `bwoc-core` stays
//! dependency-lean and every frontend can consume them:
//!
//! - a [`ColorToken`] carries both an [`Ansi`] name (terminal UIs map it to
//!   their backend's *named* colour, so the user's terminal theme is
//!   respected) and an `rgb` value (pixel UIs like egui use it directly).
//! - glyphs are `&'static str` so a TUI cell and an egui label use the same
//!   character.
//! - spacing/typography are plain `f32` (egui points; TUIs take what maps).
//!
//! ## Principles (drawn from the dashboard's own conventions)
//!
//! 1. **Redundant coding** — a state is never colour-only: every status pairs
//!    a distinct glyph with a label ([`glyph`] keeps the activity set pairwise
//!    distinct; there is a test for it).
//! 2. **Signal economy** — zero renders as "—", attention indicators appear
//!    only when non-zero (Mattaññutā: surface only what matters).
//! 3. **One meaning per colour per screen** — selection no longer reuses the
//!    idle/title yellow; muted text floors at `Gray`, never `DarkGray`-on-dark.
//! 4. **Theme respect** — terminal UIs use the `ansi` half so user themes
//!    apply; only pixel UIs use the `rgb` half.

/// The 16-colour ANSI palette by name. Terminal frontends map these to their
/// own colour type (e.g. `ratatui::style::Color`) so the terminal theme keeps
/// authority over the exact shade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ansi {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    White,
}

/// One semantic colour token: the terminal-theme-respecting [`Ansi`] name and
/// the exact RGB pixel UIs render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorToken {
    pub ansi: Ansi,
    pub rgb: (u8, u8, u8),
}

/// Semantic colour tokens. Names describe **meaning**, not hue — frontends
/// must pick by role so a palette change stays a one-file edit.
pub mod color {
    use super::{Ansi, ColorToken};

    /// Brand/interaction accent: borders of the active pane, key labels,
    /// hotkey counts, links.
    pub const ACCENT: ColorToken = ColorToken {
        ansi: Ansi::Cyan,
        rgb: (0x53, 0xC2, 0xD6),
    };
    /// Product title / banner heading.
    pub const TITLE: ColorToken = ColorToken {
        ansi: Ansi::Yellow,
        rgb: (0xE0, 0xC0, 0x60),
    };
    /// Selected-row background. Deliberately NOT yellow — selection must not
    /// share a hue with `IDLE`/`TITLE` (one meaning per colour per screen).
    pub const SELECTION_BG: ColorToken = ColorToken {
        ansi: Ansi::Blue,
        rgb: (0x2D, 0x5B, 0x9E),
    };
    /// Selected-row foreground (readable on `SELECTION_BG`).
    pub const SELECTION_FG: ColorToken = ColorToken {
        ansi: Ansi::White,
        rgb: (0xF5, 0xF5, 0xF5),
    };
    /// Activity: a session actively doing work.
    pub const WORKING: ColorToken = ColorToken {
        ansi: Ansi::Green,
        rgb: (0x9E, 0xE0, 0x93),
    };
    /// Activity: a live session with no recent output.
    pub const IDLE: ColorToken = ColorToken {
        ansi: Ansi::Yellow,
        rgb: (0xE0, 0xC0, 0x60),
    };
    /// Activity: a process that is up (distinct from `WORKING`).
    pub const RUNNING: ColorToken = ColorToken {
        ansi: Ansi::Cyan,
        rgb: (0x53, 0xC2, 0xD6),
    };
    /// Activity: a marker whose process is gone.
    pub const STALE: ColorToken = ColorToken {
        ansi: Ansi::Gray,
        rgb: (0x9A, 0x9A, 0x9A),
    };
    /// De-emphasised text that must still be readable. Floors at `Gray` —
    /// `DarkGray` on a dark terminal is near-invisible.
    pub const MUTED: ColorToken = ColorToken {
        ansi: Ansi::Gray,
        rgb: (0x9A, 0x9A, 0x9A),
    };
    /// Positive outcome / healthy.
    pub const SUCCESS: ColorToken = ColorToken {
        ansi: Ansi::Green,
        rgb: (0x9E, 0xE0, 0x93),
    };
    /// Needs attention soon (non-fatal).
    pub const WARNING: ColorToken = ColorToken {
        ansi: Ansi::Yellow,
        rgb: (0xE0, 0xC0, 0x60),
    };
    /// Errors / refusals / destructive.
    pub const DANGER: ColorToken = ColorToken {
        ansi: Ansi::Red,
        rgb: (0xE0, 0x90, 0x90),
    };
    /// The human's own messages in chat transcripts.
    pub const USER: ColorToken = ColorToken {
        ansi: Ansi::Blue,
        rgb: (0x6C, 0xB6, 0xFF),
    };
    /// System/meta lines in chat transcripts.
    pub const SYSTEM: ColorToken = MUTED;
}

/// Status glyphs. The activity set is **pairwise distinct** so state reads
/// without relying on colour (a11y) — guarded by a test below.
pub mod glyph {
    /// Session actively working.
    pub const ACTIVITY_WORKING: &str = "◉";
    /// Live session, no recent output.
    pub const ACTIVITY_IDLE: &str = "◑";
    /// Process up.
    pub const ACTIVITY_RUNNING: &str = "●";
    /// Marker present, process gone.
    pub const ACTIVITY_STALE: &str = "○";
    /// No session at all.
    pub const ACTIVITY_NONE: &str = "—";

    /// Daemon liveness, detail views: alive / not running.
    pub const RUNTIME_ALIVE: &str = "●";
    pub const RUNTIME_DEAD: &str = "○";
}

/// Spacing and typography. Values are egui points; terminal UIs take the
/// concepts (not the pixel values).
pub mod space {
    /// Vertical gap between transcript messages.
    pub const MESSAGE_GAP: f32 = 8.0;
    /// Line height as a multiple of font size for body text. ~1.4 leaves room
    /// for stacked Thai vowel/tone marks that default font metrics clip.
    pub const LINE_HEIGHT_FACTOR: f32 = 1.4;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_glyphs_are_pairwise_distinct() {
        // Redundant coding: state must read from the glyph alone.
        let set = [
            glyph::ACTIVITY_WORKING,
            glyph::ACTIVITY_IDLE,
            glyph::ACTIVITY_RUNNING,
            glyph::ACTIVITY_STALE,
            glyph::ACTIVITY_NONE,
        ];
        for (i, a) in set.iter().enumerate() {
            for b in set.iter().skip(i + 1) {
                assert_ne!(a, b, "activity glyphs must be pairwise distinct");
            }
        }
    }

    #[test]
    fn selection_does_not_reuse_idle_or_title_hue() {
        // One meaning per colour per screen: the selected row must not share
        // an ANSI hue with idle/title yellow.
        assert_ne!(color::SELECTION_BG.ansi, color::IDLE.ansi);
        assert_ne!(color::SELECTION_BG.ansi, color::TITLE.ansi);
    }

    #[test]
    fn selection_fg_differs_from_bg() {
        assert_ne!(color::SELECTION_FG.ansi, color::SELECTION_BG.ansi);
        assert_ne!(color::SELECTION_FG.rgb, color::SELECTION_BG.rgb);
    }

    #[test]
    fn muted_is_not_darkgray() {
        // Contrast floor: muted text stays readable on dark terminals.
        assert_ne!(color::MUTED.ansi, Ansi::DarkGray);
        assert_ne!(color::STALE.ansi, Ansi::DarkGray);
    }
}
