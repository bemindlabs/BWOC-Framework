//! Loop-engineering primitives (see `docs/en/LOOP-ENGINEERING.en.md`).
//!
//! The shared **Ticker** (cadence) and **Budget** (termination ceiling) the
//! goal/reconcile loops build on, so the flooring, budget-exhaustion, and
//! banner-label logic live in one place instead of being re-derived per loop.
//! Pure `std`, no dependencies (safe for the lean core crate).

use std::time::Duration;

/// The cadence between loop fires.
///
/// Today this is a single fixed interval (`Every`), floored so a zero can't spin
/// the loop. The `Adaptive`/`Cron` variants named in the spec join here when a
/// loop actually needs them — every current consumer is a fixed interval, so
/// adding them now would be an unused abstraction (Mattaññutā).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticker {
    interval: Duration,
}

impl Ticker {
    /// A fixed cadence of `secs` seconds, **floored at 1 s** — a `0` interval
    /// would spin the loop with no delay.
    pub fn every_secs(secs: u64) -> Self {
        Self {
            interval: Duration::from_secs(secs.max(1)),
        }
    }

    /// The delay to wait before the next fire.
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

/// A cross-iteration ceiling so an unattended loop provably halts.
///
/// `0` means no iteration cap — the loop still terminates on its own DoD or
/// blocked gate, but nothing bounds a loop that never reaches either, so a
/// finite cap is the safe default for an unattended loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    max_iterations: usize,
}

impl Budget {
    pub fn new(max_iterations: usize) -> Self {
        Self { max_iterations }
    }

    /// True once `iterations` reaches the cap. Always false when unbounded (`0`).
    pub fn exhausted(&self, iterations: usize) -> bool {
        self.max_iterations != 0 && iterations >= self.max_iterations
    }

    /// Label for a startup banner: `"unbounded"` or `"N iters"`.
    pub fn describe(&self) -> String {
        if self.max_iterations == 0 {
            "unbounded".to_string()
        } else {
            format!("{} iters", self.max_iterations)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticker_floors_zero_to_one_second() {
        assert_eq!(Ticker::every_secs(0).interval(), Duration::from_secs(1));
        assert_eq!(Ticker::every_secs(30).interval(), Duration::from_secs(30));
    }

    #[test]
    fn budget_exhausts_at_the_cap_but_never_when_unbounded() {
        let capped = Budget::new(3);
        assert!(!capped.exhausted(2));
        assert!(capped.exhausted(3));
        assert!(capped.exhausted(4));

        let unbounded = Budget::new(0);
        assert!(!unbounded.exhausted(1_000_000));
    }

    #[test]
    fn budget_describe() {
        assert_eq!(Budget::new(0).describe(), "unbounded");
        assert_eq!(Budget::new(20).describe(), "20 iters");
    }
}
