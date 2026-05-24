//! Self-improvement analysis — Paññā-3 retrospective triggers.
//!
//! Implements the **Bhāvanāmayā pañña** (cultivation wisdom) step from
//! AGENTS.md §8b and §11: after each session the harness reads recent
//! session-metrics history, evaluates the Paññā-3 triggers, and emits a
//! structured retrospective recommendation when thresholds are breached.
//!
//! ## Trigger thresholds (consts — configurable later via LoopConfig)
//!
//! | Trigger | Threshold | Note |
//! |---|---|---|
//! | Completion rate | < 70 % | Σ completed / Σ attempted across last N sessions |
//! | Gate pass rate  | < 70 % | Σ passed / (Σ passed + Σ failed) across last N sessions |
//! | Same feedback 3× | — | Not yet tracked; noted as follow-up (requires feedback history) |
//!
//! ## Design decisions
//!
//! - **Read-only** — this module never modifies the metrics file or agent state.
//! - **Best-effort** — any I/O or parse error silently no-ops; the run is never
//!   failed on analysis errors.
//! - **Silent when healthy** — if <2 sessions of history or no trigger fires,
//!   nothing is emitted (caller sees `RetroAnalysis { retro_recommended: false, .. }`).
//! - **Retrospective stub** — when a trigger fires the caller *may* write a stub
//!   to `<workspace>/retrospectives/YYYY-MM-DD_auto.md`.  The path is provided
//!   as `retro_stub_path` in the result; writing is the caller's responsibility.
//!
//! ## AGENTS.md §8b post-session analysis — "same feedback 3×" follow-up
//!
//! The feedback-consolidation trigger requires a separate feedback-history log
//! (not yet implemented).  This module notes the gap and skips that check.

use std::path::Path;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

/// Minimum number of sessions required before analysis runs.
/// With fewer sessions the sample is too small to be meaningful.
pub const MIN_SESSIONS: usize = 2;

/// How many recent sessions to consider.
pub const HISTORY_WINDOW: usize = 5;

/// Completion-rate threshold below which a retrospective is recommended.
/// AGENTS.md §8b: "Completion rate < 70% → flag for retrospective."
pub const COMPLETION_RATE_THRESHOLD: f64 = 0.70;

/// Gate-pass-rate threshold below which a feedback memory is recommended.
/// AGENTS.md §8b: "Gate pass rate < 70% → create feedback memory about root cause."
pub const GATE_PASS_RATE_THRESHOLD: f64 = 0.70;

// ---------------------------------------------------------------------------
// Minimal session-record deserialisation
// ---------------------------------------------------------------------------

/// The `metrics` sub-object we care about from `session-metrics.jsonl`.
///
/// We only deserialise the fields needed for trigger evaluation; all other
/// fields are ignored via `#[serde(default)]`.  This keeps the parser forward-
/// and backward-compatible as the schema evolves.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct MetricsSlice {
    tasks_attempted: u64,
    tasks_completed: u64,
    #[serde(default)]
    gates_passed: u64,
    #[serde(default)]
    gates_failed: u64,
}

/// Minimal view of a `session-metrics.jsonl` line.
#[derive(Debug, Deserialize)]
struct SessionSlice {
    metrics: MetricsSlice,
}

// ---------------------------------------------------------------------------
// Public result type
// ---------------------------------------------------------------------------

/// Outcome of the Paññā-3 retrospective analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct RetroAnalysis {
    /// `true` if at least one Paññā-3 trigger fired.
    pub retro_recommended: bool,
    /// Human-readable reasons for each trigger that fired.
    pub reasons: Vec<String>,
    /// Aggregate metrics used in the analysis (for the retro stub).
    pub completion_rate: Option<f64>,
    /// Aggregate gate pass rate used in the analysis.
    pub gate_pass_rate: Option<f64>,
    /// Number of sessions that were included in the window.
    pub sessions_analysed: usize,
}

impl RetroAnalysis {
    /// A no-op result: not enough history or all triggers passed.
    fn noop() -> Self {
        Self {
            retro_recommended: false,
            reasons: Vec::new(),
            completion_rate: None,
            gate_pass_rate: None,
            sessions_analysed: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

/// Read `session-metrics.jsonl` from `metrics_path`, analyse the last
/// [`HISTORY_WINDOW`] sessions, and return a [`RetroAnalysis`].
///
/// Never panics; all errors are silently absorbed and result in a no-op
/// analysis (Mattaññutā — don't break the run for observability).
pub fn analyse(metrics_path: &Path) -> RetroAnalysis {
    let records = match load_records(metrics_path) {
        Some(r) => r,
        None => return RetroAnalysis::noop(),
    };

    if records.len() < MIN_SESSIONS {
        return RetroAnalysis::noop();
    }

    // Take the most-recent HISTORY_WINDOW sessions.
    let window: Vec<&MetricsSlice> = records
        .iter()
        .rev()
        .take(HISTORY_WINDOW)
        .map(|s| &s.metrics)
        .collect();

    let sessions_analysed = window.len();

    // Aggregate across the window.
    let total_attempted: u64 = window.iter().map(|m| m.tasks_attempted).sum();
    let total_completed: u64 = window.iter().map(|m| m.tasks_completed).sum();
    let total_passed: u64 = window.iter().map(|m| m.gates_passed).sum();
    let total_failed: u64 = window.iter().map(|m| m.gates_failed).sum();

    let mut reasons: Vec<String> = Vec::new();

    // Completion rate trigger.
    let completion_rate: Option<f64> = if total_attempted > 0 {
        let rate = total_completed as f64 / total_attempted as f64;
        if rate < COMPLETION_RATE_THRESHOLD {
            reasons.push(format!(
                "low completion rate: {:.0}% ({}/{} tasks completed across {} sessions, threshold {}%)",
                rate * 100.0,
                total_completed,
                total_attempted,
                sessions_analysed,
                (COMPLETION_RATE_THRESHOLD * 100.0) as u32,
            ));
        }
        Some(rate)
    } else {
        None
    };

    // Gate pass rate trigger.
    let gate_denom = total_passed + total_failed;
    let gate_pass_rate: Option<f64> = if gate_denom > 0 {
        let rate = total_passed as f64 / gate_denom as f64;
        if rate < GATE_PASS_RATE_THRESHOLD {
            reasons.push(format!(
                "low gate pass rate: {:.0}% ({}/{} gates passed across {} sessions, threshold {}%)",
                rate * 100.0,
                total_passed,
                gate_denom,
                sessions_analysed,
                (GATE_PASS_RATE_THRESHOLD * 100.0) as u32,
            ));
        }
        Some(rate)
    } else {
        None
    };

    // NOTE: "same feedback 3×" trigger is not yet implemented.
    // It requires a separate feedback-history log (follow-up: HV2-4 or later).

    let retro_recommended = !reasons.is_empty();

    RetroAnalysis {
        retro_recommended,
        reasons,
        completion_rate,
        gate_pass_rate,
        sessions_analysed,
    }
}

// ---------------------------------------------------------------------------
// Retrospective stub
// ---------------------------------------------------------------------------

/// Build the content for a retrospective stub file.
///
/// Called only when `analysis.retro_recommended` is `true`.
pub fn build_retro_stub(analysis: &RetroAnalysis, agent_id: &str, date: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("# {date} — Auto-retrospective ({agent_id})\n\n"));
    s.push_str("*Generated by bwoc-harness Paññā-3 analysis. Review and act on findings.*\n\n");
    s.push_str("## Triggers fired\n\n");
    for reason in &analysis.reasons {
        s.push_str(&format!("- {reason}\n"));
    }
    s.push_str("\n## Metrics window\n\n");
    s.push_str(&format!(
        "Sessions analysed: {}\n\n",
        analysis.sessions_analysed
    ));
    if let Some(cr) = analysis.completion_rate {
        s.push_str(&format!("Completion rate: {:.1}%\n", cr * 100.0));
    }
    if let Some(gpr) = analysis.gate_pass_rate {
        s.push_str(&format!("Gate pass rate: {:.1}%\n", gpr * 100.0));
    }
    s.push_str("\n## Action items\n\n");
    s.push_str("- [ ] Investigate root cause for each trigger above\n");
    s.push_str("- [ ] Create or update a feedback memory in `memories/`\n");
    s.push_str("- [ ] Adjust task scoping or gate configuration if appropriate\n");
    s
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse `session-metrics.jsonl` and return the records.
/// Returns `None` on any I/O or empty-file condition.
fn load_records(path: &Path) -> Option<Vec<SessionSlice>> {
    let content = std::fs::read_to_string(path).ok()?;
    let records: Vec<SessionSlice> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            // Silently skip malformed lines — best-effort.
            serde_json::from_str::<SessionSlice>(line).ok()
        })
        .collect();

    if records.is_empty() {
        None
    } else {
        Some(records)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_metrics(dir: &TempDir, lines: &[&str]) -> std::path::PathBuf {
        let path = dir.path().join("session-metrics.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    // Helper: build a minimal valid JSONL line with explicit metrics fields.
    fn session_line(attempted: u64, completed: u64, passed: u64, failed: u64) -> String {
        format!(
            r#"{{"sessionId":"s","agentId":"a","startedAt":"2026-01-01T00:00:00Z","endedAt":"2026-01-01T00:01:00Z","metrics":{{"tasksAttempted":{attempted},"tasksCompleted":{completed},"tasksFailed":0,"gatesPassed":{passed},"gatesFailed":{failed},"revisionCycles":0,"memoriesCreated":0,"memoriesUpdated":0,"memoriesRemoved":0}},"discoveries":[]}}"#
        )
    }

    // ── Trigger: completion rate < 70% ────────────────────────────────────────

    #[test]
    fn completion_low_fires_trigger() {
        let tmp = TempDir::new().unwrap();
        // 2 sessions: 1 attempted, 0 completed each → completion 0%
        let lines = [session_line(1, 0, 4, 0), session_line(1, 0, 4, 0)];
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        assert!(result.retro_recommended, "completion trigger should fire");
        assert!(
            result.reasons.iter().any(|r| r.contains("completion")),
            "reason should mention completion: {:?}",
            result.reasons
        );
        assert_eq!(result.sessions_analysed, 2);
        assert!(result.completion_rate.unwrap() < COMPLETION_RATE_THRESHOLD);
    }

    // ── Trigger: gate pass rate < 70% ─────────────────────────────────────────

    #[test]
    fn gate_pass_low_fires_trigger() {
        let tmp = TempDir::new().unwrap();
        // 2 sessions: gates 1 passed, 4 failed each → pass rate 20%
        let lines = [session_line(1, 1, 1, 4), session_line(1, 1, 1, 4)];
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        assert!(result.retro_recommended, "gate trigger should fire");
        assert!(
            result.reasons.iter().any(|r| r.contains("gate")),
            "reason should mention gate: {:?}",
            result.reasons
        );
        assert!(result.gate_pass_rate.unwrap() < GATE_PASS_RATE_THRESHOLD);
    }

    // ── Healthy history — no trigger ──────────────────────────────────────────

    #[test]
    fn healthy_history_no_trigger() {
        let tmp = TempDir::new().unwrap();
        // 5 healthy sessions: completion 100%, gate pass 100%
        let lines: Vec<String> = (0..5).map(|_| session_line(2, 2, 5, 0)).collect();
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        assert!(!result.retro_recommended);
        assert!(result.reasons.is_empty());
    }

    // ── Fewer than MIN_SESSIONS — no-op ──────────────────────────────────────

    #[test]
    fn less_than_min_sessions_noop() {
        let tmp = TempDir::new().unwrap();
        // Only 1 session — below MIN_SESSIONS (2).
        let lines = [session_line(1, 0, 0, 5)]; // would trigger if analysed
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        assert!(!result.retro_recommended, "should be noop with <2 sessions");
        assert_eq!(result.sessions_analysed, 0);
    }

    // ── Gate pass rate trigger (edge: exactly at threshold) ───────────────────

    #[test]
    fn gate_pass_exactly_at_threshold_no_trigger() {
        let tmp = TempDir::new().unwrap();
        // Exactly 70%: 7 passed, 3 failed per session.
        let lines = [session_line(1, 1, 7, 3), session_line(1, 1, 7, 3)];
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        // Exactly at threshold (not below) → no trigger.
        assert!(!result.retro_recommended);
    }

    // ── Malformed / empty lines don't panic ───────────────────────────────────

    #[test]
    fn malformed_lines_no_panic() {
        let tmp = TempDir::new().unwrap();
        let lines = [
            "not json at all",
            "",
            r#"{"partial": true}"#, // missing metrics field — ignored
            &session_line(1, 0, 1, 0),
            &session_line(1, 0, 1, 0),
        ];
        let path = write_metrics(&tmp, &lines);
        // Must not panic.
        let result = analyse(&path);
        // The 2 valid sessions with 0 completion should trigger.
        assert!(result.retro_recommended);
    }

    // ── Empty file — no-op ────────────────────────────────────────────────────

    #[test]
    fn empty_file_noop() {
        let tmp = TempDir::new().unwrap();
        let path = write_metrics(&tmp, &[]);
        let result = analyse(&path);
        assert!(!result.retro_recommended);
    }

    // ── Missing file — no-op ─────────────────────────────────────────────────

    #[test]
    fn missing_file_noop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.jsonl");
        let result = analyse(&path);
        assert!(!result.retro_recommended);
    }

    // ── HISTORY_WINDOW cap — only last 5 sessions used ────────────────────────

    #[test]
    fn history_window_caps_at_five() {
        let tmp = TempDir::new().unwrap();
        // 8 sessions: first 3 are bad (0% completion), last 5 are healthy (100%).
        // With window=5 only the healthy tail is seen → no trigger.
        let mut lines: Vec<String> = (0..3).map(|_| session_line(1, 0, 5, 0)).collect();
        lines.extend((0..5).map(|_| session_line(1, 1, 5, 0)));
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        assert!(
            !result.retro_recommended,
            "window cap should exclude old bad sessions"
        );
        assert_eq!(result.sessions_analysed, HISTORY_WINDOW);
    }

    // ── Both triggers fire simultaneously ────────────────────────────────────

    #[test]
    fn both_triggers_fire() {
        let tmp = TempDir::new().unwrap();
        // 0% completion, 20% gate pass rate.
        let lines = [session_line(2, 0, 1, 4), session_line(2, 0, 1, 4)];
        let path = write_metrics(&tmp, &lines.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let result = analyse(&path);

        assert!(result.retro_recommended);
        assert_eq!(
            result.reasons.len(),
            2,
            "both triggers should fire: {:?}",
            result.reasons
        );
    }

    // ── build_retro_stub produces expected sections ───────────────────────────

    #[test]
    fn retro_stub_contains_expected_sections() {
        let analysis = RetroAnalysis {
            retro_recommended: true,
            reasons: vec!["low completion rate: 50%".to_string()],
            completion_rate: Some(0.5),
            gate_pass_rate: Some(0.9),
            sessions_analysed: 3,
        };
        let stub = build_retro_stub(&analysis, "agent-oracle", "2026-05-24");
        assert!(stub.contains("Auto-retrospective"));
        assert!(stub.contains("Triggers fired"));
        assert!(stub.contains("low completion rate"));
        assert!(stub.contains("Metrics window"));
        assert!(stub.contains("Action items"));
    }
}
