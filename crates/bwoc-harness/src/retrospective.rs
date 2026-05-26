//! Run-end retrospective (HV2-3) — closes the Paññā-3 loop.
//!
//! At the end of a run the harness has a [`SessionRecord`] (the same record it
//! appends to `session-metrics.jsonl`).  Until now that data was write-only:
//! telemetry was emitted and never read back, and `eval` ran only offline.
//! This module reads the record back **into the same run** and fires the
//! AGENTS.md §8b self-improvement triggers, turning a passive log into the
//! Bhāvanāmayā step of the Paññā-3 loop:
//!
//! - **Sutamayā** (learning) — the metrics schema the operator already reviews.
//! - **Cintāmayā** (reflection) — [`Retrospective::analyze`] synthesises the
//!   pattern: which §8b threshold did this run cross?
//! - **Bhāvanāmayā** (practice) — the surfaced suggestions ("save a feedback
//!   memory", "re-read the affected suta") the operator then acts on.
//!
//! ## Observe, don't drive
//!
//! A retrospective only *surfaces* adjustments — it never mutates the policy,
//! prompt, or any run state.  The safety pipeline stays the single authority on
//! what an agent may do; self-improvement proposes, the operator disposes.

use serde::{Deserialize, Serialize};

use crate::telemetry::SessionRecord;

/// Thresholds for the §8b post-session triggers.  Defaults mirror
/// `AGENTS.md §8b`: completion rate and gate-pass rate both 70%.
#[derive(Debug, Clone)]
pub struct RetroThresholds {
    /// Completion rate below this flags the run for retrospective.
    pub min_completion_rate: f64,
    /// Gate-pass rate below this suggests a root-cause feedback memory.
    pub min_gate_pass_rate: f64,
}

impl Default for RetroThresholds {
    fn default() -> Self {
        Self {
            min_completion_rate: 0.70,
            min_gate_pass_rate: 0.70,
        }
    }
}

/// A §8b trigger that fired during analysis.  Each carries the observed rate
/// and the threshold it fell under, so the surfaced message is self-contained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "trigger")]
pub enum Trigger {
    /// Completion rate `< min_completion_rate` (AGENTS.md §8b: "flag for
    /// retrospective").
    LowCompletionRate { rate: f64, threshold: f64 },
    /// Gate-pass rate `< min_gate_pass_rate` (AGENTS.md §8b: "create feedback
    /// memory about root cause").
    LowGatePassRate { rate: f64, threshold: f64 },
}

impl Trigger {
    /// The Paññā-staged suggestion this trigger surfaces — Bhāvanāmayā, the
    /// operator's next concrete move.  Observe-don't-drive: a sentence, not an
    /// action.
    pub fn suggestion(&self) -> String {
        match self {
            Trigger::LowCompletionRate { rate, threshold } => format!(
                "completion rate {:.0}% < {:.0}% — flag for retrospective; \
                 re-read the task's suta (Sutamayā) before retrying.",
                rate * 100.0,
                threshold * 100.0
            ),
            Trigger::LowGatePassRate { rate, threshold } => format!(
                "gate-pass rate {:.0}% < {:.0}% — synthesise the failing pattern \
                 (Cintāmayā) and save a feedback memory about the root cause \
                 (Bhāvanāmayā).",
                rate * 100.0,
                threshold * 100.0
            ),
        }
    }
}

/// Outcome of a run-end retrospective.
///
/// Carries observations (always present, descriptive) and triggers (the §8b
/// thresholds that were crossed).  It never holds an instruction the harness
/// will execute — only text for the operator.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Retrospective {
    /// §8b thresholds crossed this run.
    pub triggers: Vec<Trigger>,
    /// Descriptive notes (rates, caveats) regardless of whether a trigger fired.
    pub observations: Vec<String>,
}

impl Retrospective {
    /// Analyse a finished run's record against `thresholds`.
    ///
    /// Pure: reads the record, computes the §8b rates, and returns the
    /// retrospective.  Rates whose denominator is zero (no tasks counted, no
    /// gates run) are skipped with a caveat rather than treated as `0%` — a run
    /// that ran no gates did not "fail" them.
    pub fn analyze(record: &SessionRecord, thresholds: &RetroThresholds) -> Self {
        let mut triggers = Vec::new();
        let mut observations = Vec::new();

        // ── Completion rate (AGENTS.md §8b) ──────────────────────────────────
        let attempted = record.metrics.tasks_attempted;
        if attempted == 0 {
            observations.push(
                "completion rate: n/a (no tasks counted this run — caller did \
                 not set tasksAttempted)"
                    .to_string(),
            );
        } else {
            let rate = f64::from(record.metrics.tasks_completed) / f64::from(attempted);
            observations.push(format!(
                "completion rate: {:.0}% ({}/{})",
                rate * 100.0,
                record.metrics.tasks_completed,
                attempted
            ));
            if rate < thresholds.min_completion_rate {
                triggers.push(Trigger::LowCompletionRate {
                    rate,
                    threshold: thresholds.min_completion_rate,
                });
            }
        }

        // ── Gate-pass rate (AGENTS.md §8b) ───────────────────────────────────
        let passed = record.metrics.gates_passed;
        let failed = record.metrics.gates_failed;
        let total_gates = passed + failed;
        if total_gates == 0 {
            observations.push("gate-pass rate: n/a (no gates run this run)".to_string());
        } else {
            let rate = passed as f64 / total_gates as f64;
            observations.push(format!(
                "gate-pass rate: {:.0}% ({}/{})",
                rate * 100.0,
                passed,
                total_gates
            ));
            if rate < thresholds.min_gate_pass_rate {
                triggers.push(Trigger::LowGatePassRate {
                    rate,
                    threshold: thresholds.min_gate_pass_rate,
                });
            }
        }

        // §8b intends these triggers over an aggregate of 5+ sessions; a
        // single-run retrospective surfaces the signal early without claiming
        // statistical weight.
        if !triggers.is_empty() {
            observations.push(
                "note: §8b thresholds are intended over 5+ sessions — treat a \
                 single-run trigger as a hint, not a verdict."
                    .to_string(),
            );
        }

        Self {
            triggers,
            observations,
        }
    }

    /// Whether any §8b trigger fired.
    pub fn has_triggers(&self) -> bool {
        !self.triggers.is_empty()
    }

    /// Human-readable block for stderr at run end.  Observations always; a
    /// suggestions list only when triggers fired.
    pub fn render(&self) -> String {
        let mut out = String::from("── run-end retrospective (Paññā-3) ──\n");
        for obs in &self.observations {
            out.push_str(&format!("  · {obs}\n"));
        }
        if self.has_triggers() {
            out.push_str("  suggested adjustments (surfaced, not applied):\n");
            for t in &self.triggers {
                out.push_str(&format!("    → {}\n", t.suggestion()));
            }
        } else {
            out.push_str("  no §8b thresholds crossed — nothing to surface.\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{AgentMetrics, SessionRecord};

    fn record_with(metrics: AgentMetrics) -> SessionRecord {
        SessionRecord {
            session_id: "s1".to_string(),
            agent_id: "agent-test".to_string(),
            started_at: "2026-05-26T00:00:00Z".to_string(),
            ended_at: "2026-05-26T00:01:00Z".to_string(),
            metrics,
            discoveries: Vec::new(),
            harness: None,
        }
    }

    #[test]
    fn low_gate_pass_rate_fires_trigger() {
        let m = AgentMetrics {
            gates_passed: 1,
            gates_failed: 3, // 25% pass < 70%
            ..Default::default()
        };
        let retro = Retrospective::analyze(&record_with(m), &RetroThresholds::default());
        assert!(retro.has_triggers());
        assert!(matches!(retro.triggers[0], Trigger::LowGatePassRate { .. }));
        assert!(retro.render().contains("feedback memory"));
    }

    #[test]
    fn healthy_run_fires_nothing() {
        let m = AgentMetrics {
            tasks_attempted: 1,
            tasks_completed: 1,
            gates_passed: 4,
            gates_failed: 0,
            ..Default::default()
        };
        let retro = Retrospective::analyze(&record_with(m), &RetroThresholds::default());
        assert!(!retro.has_triggers());
        assert!(retro.render().contains("nothing to surface"));
    }

    #[test]
    fn low_completion_rate_fires_trigger() {
        let m = AgentMetrics {
            tasks_attempted: 10,
            tasks_completed: 6, // 60% < 70%
            ..Default::default()
        };
        let retro = Retrospective::analyze(&record_with(m), &RetroThresholds::default());
        assert!(
            retro
                .triggers
                .iter()
                .any(|t| matches!(t, Trigger::LowCompletionRate { .. }))
        );
    }

    #[test]
    fn zero_denominators_skip_without_false_trigger() {
        // No tasks counted, no gates run — must NOT report 0% and fire.
        let retro = Retrospective::analyze(
            &record_with(AgentMetrics::default()),
            &RetroThresholds::default(),
        );
        assert!(!retro.has_triggers(), "empty run is not a failed run");
        assert!(retro.observations.iter().any(|o| o.contains("n/a")));
    }
}
