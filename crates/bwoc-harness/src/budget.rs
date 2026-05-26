//! Per-run budget governance (HV2-6).
//!
//! A hard gate atop the existing token-pressure ladder: the loop already
//! *warns* on unvetted models and *switches* to a larger-context model under
//! pressure; this is the final stop — when cumulative usage crosses a token or
//! cost budget, the run aborts with [`HarnessError::BudgetExceeded`] rather than
//! spending more.  *Mattaññutā at runtime — the right amount, enforced.*
//!
//! Token budgets work on any path.  Cost budgets need a price
//! (`cost_per_1m_tokens`); without one, only the token budget applies — cost is
//! left unknown rather than assumed zero.  The streaming path can be budgeted
//! now that HV2-7 (BWOC-9) surfaces streaming usage; before it, only the
//! non-streaming path reported tokens.

use crate::error::HarnessError;

/// Per-run budget limits.  All-`None` (the default) means no gate.
#[derive(Debug, Clone, Default)]
pub struct BudgetConfig {
    /// Hard cap on cumulative tokens (prompt + completion) across the run.
    pub max_tokens: Option<u64>,
    /// Hard cap on cumulative cost in the operator's currency unit (e.g. USD).
    /// Only enforced when `cost_per_1m_tokens` is also set.
    pub max_cost: Option<f64>,
    /// Price per 1,000,000 tokens, used to derive cost from token usage.
    pub cost_per_1m_tokens: Option<f64>,
}

impl BudgetConfig {
    /// `true` when no limit is configured — the loop can skip the check.
    pub fn is_unlimited(&self) -> bool {
        self.max_tokens.is_none() && self.max_cost.is_none()
    }

    /// Estimated cost for `total_tokens`, if a price is configured.
    pub fn cost(&self, total_tokens: u64) -> Option<f64> {
        self.cost_per_1m_tokens
            .map(|price| (total_tokens as f64 / 1_000_000.0) * price)
    }

    /// Check cumulative `total_tokens` against the configured limits.
    ///
    /// Returns `Err(HarnessError::BudgetExceeded)` on the first breached limit
    /// (token budget first, then cost).  `Ok(())` when within budget or
    /// unlimited.
    pub fn check(&self, total_tokens: u64) -> Result<(), HarnessError> {
        if let Some(max) = self.max_tokens {
            if total_tokens > max {
                return Err(HarnessError::BudgetExceeded {
                    kind: "token",
                    used: total_tokens as f64,
                    limit: max as f64,
                });
            }
        }
        if let (Some(max_cost), Some(cost)) = (self.max_cost, self.cost(total_tokens)) {
            if cost > max_cost {
                return Err(HarnessError::BudgetExceeded {
                    kind: "cost",
                    used: cost,
                    limit: max_cost,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_by_default_never_trips() {
        let b = BudgetConfig::default();
        assert!(b.is_unlimited());
        assert!(b.check(u64::MAX).is_ok());
    }

    #[test]
    fn token_budget_trips_when_exceeded() {
        let b = BudgetConfig {
            max_tokens: Some(1000),
            ..Default::default()
        };
        assert!(b.check(1000).is_ok(), "at the limit is within budget");
        let err = b.check(1001).unwrap_err();
        assert!(matches!(
            err,
            HarnessError::BudgetExceeded { kind: "token", .. }
        ));
    }

    #[test]
    fn cost_budget_needs_a_price() {
        // max_cost without a price → cost never computed → token-only behaviour.
        let b = BudgetConfig {
            max_cost: Some(0.01),
            cost_per_1m_tokens: None,
            ..Default::default()
        };
        assert_eq!(b.cost(1_000_000), None);
        assert!(b.check(10_000_000).is_ok(), "no price → cost gate inactive");
    }

    #[test]
    fn cost_budget_trips_with_a_price() {
        let b = BudgetConfig {
            max_cost: Some(1.0),
            cost_per_1m_tokens: Some(2.0), // $2 per 1M tokens
            ..Default::default()
        };
        // 400k tokens → $0.80, within $1.00.
        assert!(b.check(400_000).is_ok());
        // 600k tokens → $1.20, over $1.00.
        let err = b.check(600_000).unwrap_err();
        assert!(matches!(
            err,
            HarnessError::BudgetExceeded { kind: "cost", .. }
        ));
    }
}
