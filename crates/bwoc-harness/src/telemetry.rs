//! Telemetry — per-turn metrics and session-level observability.
//!
//! **P3 component** — Satipaṭṭhāna 4 (four foundations of mindfulness applied
//! to the harness's own operation):
//!
//! | Foundation | Metric |
//! |---|---|
//! | Kāyānupassanā (body/process) | latency ms, context tokens |
//! | Vedanānupassanā (sensation/I/O) | tokens in/out |
//! | Cittānupassanā (mind state) | tool-call count |
//! | Dhammānupassanā (rules) | denial count, gate pass/fail |
//!
//! ## Session-metrics schema
//!
//! One `SessionRecord` is appended to `session-metrics.jsonl` per session.
//! The shape is **additive** to the template `AGENTS.md §8b` schema — all
//! existing required fields are preserved; this module only adds the
//! `harness` extension key (optional, ignored by readers that don't know it).
//!
//! ```jsonc
//! {
//!   "sessionId":   "sess-2026-05-23-001",
//!   "agentId":     "agent-oracle",
//!   "startedAt":   "2026-05-23T10:00:00Z",
//!   "endedAt":     "2026-05-23T10:05:00Z",
//!   "metrics": {
//!     "tasksAttempted": 1,
//!     "tasksCompleted": 1,
//!     "tasksFailed":    0,
//!     "gatesPassed":    4,
//!     "gatesFailed":    0,
//!     "revisionCycles": 0,
//!     "memoriesCreated": 0,
//!     "memoriesUpdated": 0,
//!     "memoriesRemoved": 0
//!   },
//!   "discoveries": [],
//!   "harness": {
//!     "turns": [...],
//!     "totals": { ... }
//!   }
//! }
//! ```
//!
//! ## OpenTelemetry export (optional)
//!
//! Compile with `--features otel` to enable OTEL span export.  The default
//! build has **no OTEL dependency** — a stub exporter is used instead.
//!
//! ## Secrets
//!
//! Telemetry MUST NOT include secret values.  The `TurnMetrics` struct only
//! carries counts and durations — never env-var values, command arguments
//! that may contain tokens, or any string from the credential broker.

use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Per-turn metrics
// ---------------------------------------------------------------------------

/// Metrics collected for a single agent turn (one model call + tool dispatch).
///
/// All fields are counts or durations — no string payloads, so no risk of
/// accidentally capturing secrets.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TurnMetrics {
    /// Turn index (1-based).
    pub turn: u32,
    /// Tokens sent to the model in this turn (prompt tokens).
    pub tokens_in: u32,
    /// Tokens returned by the model (completion tokens).
    pub tokens_out: u32,
    /// Wall-clock latency for the model call, in milliseconds.
    pub latency_ms: u64,
    /// Number of tool calls the model requested in this turn.
    pub tool_calls: u32,
    /// Number of tool calls denied by the guardrail or permission layers.
    pub denials: u32,
    /// Number of verification gates that passed in this turn.
    pub gates_passed: u32,
    /// Number of verification gates that failed in this turn.
    pub gates_failed: u32,
    /// Total context-window tokens at the end of this turn (prompt + history).
    pub context_tokens: u32,
    /// `1` if a token-pressure–driven model switch occurred at the start of
    /// this turn; `0` otherwise.  Distinct from error-based fallback switches.
    pub token_pressure_switch: u32,
}

// ---------------------------------------------------------------------------
// Session-level aggregates
// ---------------------------------------------------------------------------

/// Totals accumulated across all turns in one session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SessionTotals {
    pub turns: u32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: u64,
    pub denials: u64,
    pub gates_passed: u64,
    pub gates_failed: u64,
    /// Total number of token-pressure–driven model switches across all turns.
    /// Distinct from error-based fallback switches (not counted here).
    pub token_pressure_switches: u64,
}

impl SessionTotals {
    pub fn accumulate(&mut self, turn: &TurnMetrics) {
        self.turns += 1;
        self.tokens_in += u64::from(turn.tokens_in);
        self.tokens_out += u64::from(turn.tokens_out);
        self.tool_calls += u64::from(turn.tool_calls);
        self.denials += u64::from(turn.denials);
        self.gates_passed += u64::from(turn.gates_passed);
        self.gates_failed += u64::from(turn.gates_failed);
        self.token_pressure_switches += u64::from(turn.token_pressure_switch);
    }
}

// ---------------------------------------------------------------------------
// Harness extension block (nested inside the AGENTS.md §8b schema)
// ---------------------------------------------------------------------------

/// Harness-specific metrics block, nested under `"harness"` in the record.
/// Readers that don't understand `"harness"` safely ignore it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HarnessBlock {
    /// Per-turn breakdowns.
    pub turns: Vec<TurnMetrics>,
    /// Aggregated totals.
    pub totals: SessionTotals,
}

// ---------------------------------------------------------------------------
// Session record — compatible with AGENTS.md §8b shape
// ---------------------------------------------------------------------------

/// The subset of AGENTS.md §8b `metrics` that we write.
///
/// All fields are required by the schema; this struct is the canonical
/// carrier so callers don't have to construct raw JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentMetrics {
    pub tasks_attempted: u32,
    pub tasks_completed: u32,
    pub tasks_failed: u32,
    pub gates_passed: u64,
    pub gates_failed: u64,
    pub revision_cycles: u32,
    pub memories_created: u32,
    pub memories_updated: u32,
    pub memories_removed: u32,
}

/// One record appended to `session-metrics.jsonl`.
///
/// Schema is **additive** to AGENTS.md §8b.  All required fields are present.
/// The `harness` field is the extension block — it is `None` if not running
/// under the harness, and is safely ignored by readers that pre-date P3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    pub agent_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub metrics: AgentMetrics,
    pub discoveries: Vec<serde_json::Value>,
    /// Harness-specific extension — absent in non-harness sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<HarnessBlock>,
}

// ---------------------------------------------------------------------------
// Session accumulator (mutable state during a session)
// ---------------------------------------------------------------------------

/// Accumulates telemetry during an agent session and appends the final
/// record to `session-metrics.jsonl` on [`Telemetry::finish`].
pub struct Telemetry {
    pub session_id: String,
    pub agent_id: String,
    started_at: String,
    /// Monotonic start for latency measurements.
    started_instant: Instant,
    turns: Vec<TurnMetrics>,
    totals: SessionTotals,
    /// High-level agent metrics (tasks/gates/memories).
    pub agent: AgentMetrics,
}

impl Telemetry {
    /// Start a new telemetry session.
    pub fn new(session_id: impl Into<String>, agent_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            agent_id: agent_id.into(),
            started_at: utc_now(),
            started_instant: Instant::now(),
            turns: Vec::new(),
            totals: SessionTotals::default(),
            agent: AgentMetrics::default(),
        }
    }

    /// Record a completed turn's metrics.
    pub fn record_turn(&mut self, turn: TurnMetrics) {
        self.totals.accumulate(&turn);
        // Mirror gate counts into the agent-level aggregates so the
        // AGENTS.md §8b required fields stay accurate.
        self.agent.gates_passed += turn.gates_passed as u64;
        self.agent.gates_failed += turn.gates_failed as u64;
        self.turns.push(turn);
    }

    /// Build the final [`SessionRecord`] without writing it anywhere.
    /// Useful for testing the schema shape without file I/O.
    pub fn build_record(&self) -> SessionRecord {
        let harness = HarnessBlock {
            turns: self.turns.clone(),
            totals: self.totals.clone(),
        };
        SessionRecord {
            session_id: self.session_id.clone(),
            agent_id: self.agent_id.clone(),
            started_at: self.started_at.clone(),
            ended_at: utc_now(),
            metrics: self.agent.clone(),
            discoveries: Vec::new(),
            harness: Some(harness),
        }
    }

    /// Append one JSON-lines record to the given `session-metrics.jsonl` file.
    ///
    /// Opens the file in append mode, so existing records are preserved.
    /// Returns `Ok(())` on success or an I/O error if the file cannot be
    /// opened or written.
    pub fn finish(&self, sink: &Path) -> std::io::Result<()> {
        let record = self.build_record();
        let line = serde_json::to_string(&record).map_err(std::io::Error::other)?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(sink)?;

        writeln!(file, "{line}")?;

        // OTEL export — only active under the `otel` feature flag.
        #[cfg(feature = "otel")]
        export_otel_span(&record);

        Ok(())
    }

    /// Elapsed milliseconds since this session started.
    pub fn elapsed_ms(&self) -> u64 {
        self.started_instant.elapsed().as_millis() as u64
    }
}

// ---------------------------------------------------------------------------
// Turn builder — measures latency automatically
// ---------------------------------------------------------------------------

/// A builder for [`TurnMetrics`] that measures latency from construction to
/// [`TurnBuilder::finish`].
pub struct TurnBuilder {
    turn: u32,
    start: Instant,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub tool_calls: u32,
    pub denials: u32,
    pub gates_passed: u32,
    pub gates_failed: u32,
    pub context_tokens: u32,
    /// Set to `1` when a token-pressure–driven model switch occurred before
    /// this turn's provider call.  `0` = no switch this turn.
    pub token_pressure_switch: u32,
}

impl TurnBuilder {
    pub fn new(turn: u32) -> Self {
        Self {
            turn,
            start: Instant::now(),
            tokens_in: 0,
            tokens_out: 0,
            tool_calls: 0,
            denials: 0,
            gates_passed: 0,
            gates_failed: 0,
            context_tokens: 0,
            token_pressure_switch: 0,
        }
    }

    /// Build the final [`TurnMetrics`], capturing elapsed time automatically.
    pub fn finish(self) -> TurnMetrics {
        TurnMetrics {
            turn: self.turn,
            tokens_in: self.tokens_in,
            tokens_out: self.tokens_out,
            latency_ms: self.start.elapsed().as_millis() as u64,
            tool_calls: self.tool_calls,
            denials: self.denials,
            gates_passed: self.gates_passed,
            gates_failed: self.gates_failed,
            context_tokens: self.context_tokens,
            token_pressure_switch: self.token_pressure_switch,
        }
    }
}

// ---------------------------------------------------------------------------
// OTEL stub — active only under `--features otel`
// ---------------------------------------------------------------------------

#[cfg(feature = "otel")]
fn export_otel_span(record: &SessionRecord) {
    // TODO(P4): replace with a real opentelemetry exporter.
    // For now this is a compile-gated stub so the feature flag works without
    // pulling the full OTEL crate graph into the default build.
    eprintln!(
        "[otel-stub] would export span for session {} agent {}",
        record.session_id, record.agent_id
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn utc_now() -> String {
    // ISO 8601 UTC timestamp.  `std::time` has no timezone, so we produce a
    // UTC timestamp manually from SystemTime.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format: YYYY-MM-DDTHH:MM:SSZ (no fractional seconds — sufficient for metrics)
    let s = secs;
    let (y, mo, d, h, mi, sec) = epoch_to_ymd_hms(s);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z")
}

fn epoch_to_ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let hour = hours % 24;
    let days = hours / 24;

    // Gregorian calendar computation.
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let months = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0u64;
    for (i, &days_in_month) in months.iter().enumerate() {
        let dim = if i == 1 && is_leap(year) {
            29
        } else {
            days_in_month
        };
        if remaining < dim {
            month = i as u64 + 1;
            break;
        }
        remaining -= dim;
    }
    let day = remaining + 1;

    (year, month, day, hour, min, sec)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── TurnMetrics: shape and defaults ─────────────────────────────────────

    #[test]
    fn turn_metrics_default_all_zero() {
        let m = TurnMetrics::default();
        assert_eq!(m.turn, 0);
        assert_eq!(m.tokens_in, 0);
        assert_eq!(m.tokens_out, 0);
        assert_eq!(m.latency_ms, 0);
        assert_eq!(m.tool_calls, 0);
        assert_eq!(m.denials, 0);
        assert_eq!(m.gates_passed, 0);
        assert_eq!(m.gates_failed, 0);
        assert_eq!(m.context_tokens, 0);
    }

    // ── TurnBuilder measures latency ─────────────────────────────────────────

    #[test]
    fn turn_builder_sets_fields_correctly() {
        let mut b = TurnBuilder::new(1);
        b.tokens_in = 100;
        b.tokens_out = 50;
        b.tool_calls = 2;
        b.denials = 1;
        b.gates_passed = 3;
        b.gates_failed = 0;
        b.context_tokens = 512;
        let m = b.finish();
        assert_eq!(m.turn, 1);
        assert_eq!(m.tokens_in, 100);
        assert_eq!(m.tokens_out, 50);
        assert_eq!(m.tool_calls, 2);
        assert_eq!(m.denials, 1);
        assert_eq!(m.gates_passed, 3);
        assert_eq!(m.gates_failed, 0);
        assert_eq!(m.context_tokens, 512);
        // latency_ms is at least 0 (can't assert exact value in unit tests).
    }

    // ── SessionTotals accumulation ────────────────────────────────────────────

    #[test]
    fn totals_accumulate_across_turns() {
        let mut totals = SessionTotals::default();
        let t1 = TurnMetrics {
            turn: 1,
            tokens_in: 100,
            tokens_out: 50,
            tool_calls: 2,
            denials: 1,
            gates_passed: 2,
            gates_failed: 0,
            latency_ms: 200,
            context_tokens: 300,
            token_pressure_switch: 0,
        };
        let t2 = TurnMetrics {
            turn: 2,
            tokens_in: 200,
            tokens_out: 100,
            tool_calls: 1,
            denials: 0,
            gates_passed: 1,
            gates_failed: 1,
            latency_ms: 150,
            context_tokens: 600,
            token_pressure_switch: 0,
        };
        totals.accumulate(&t1);
        totals.accumulate(&t2);
        assert_eq!(totals.turns, 2);
        assert_eq!(totals.tokens_in, 300);
        assert_eq!(totals.tokens_out, 150);
        assert_eq!(totals.tool_calls, 3);
        assert_eq!(totals.denials, 1);
        assert_eq!(totals.gates_passed, 3);
        assert_eq!(totals.gates_failed, 1);
    }

    // ── Session record shape is additive to AGENTS.md §8b ────────────────────

    #[test]
    fn session_record_serializes_required_fields() {
        let mut telem = Telemetry::new("sess-test-001", "agent-oracle");
        let m = TurnMetrics {
            turn: 1,
            tokens_in: 100,
            tokens_out: 50,
            tool_calls: 1,
            denials: 0,
            gates_passed: 2,
            gates_failed: 0,
            latency_ms: 300,
            context_tokens: 500,
            token_pressure_switch: 0,
        };
        telem.record_turn(m);
        telem.agent.tasks_attempted = 1;
        telem.agent.tasks_completed = 1;

        let record = telem.build_record();

        // All AGENTS.md §8b required fields must be present.
        let json = serde_json::to_value(&record).unwrap();
        assert!(json.get("sessionId").is_some(), "missing sessionId");
        assert!(json.get("agentId").is_some(), "missing agentId");
        assert!(json.get("startedAt").is_some(), "missing startedAt");
        assert!(json.get("endedAt").is_some(), "missing endedAt");
        assert!(json.get("metrics").is_some(), "missing metrics");
        assert!(json.get("discoveries").is_some(), "missing discoveries");

        // Verify agent-level metrics keys (camelCase — per §8b schema).
        let metrics = &json["metrics"];
        assert!(metrics.get("tasksAttempted").is_some());
        assert!(metrics.get("tasksCompleted").is_some());
        assert!(metrics.get("tasksFailed").is_some());
        assert!(metrics.get("gatesPassed").is_some());
        assert!(metrics.get("gatesFailed").is_some());
        assert!(metrics.get("revisionCycles").is_some());
        assert!(metrics.get("memoriesCreated").is_some());
        assert!(metrics.get("memoriesUpdated").is_some());
        assert!(metrics.get("memoriesRemoved").is_some());

        // Harness extension is present (additive, not breaking).
        assert!(json.get("harness").is_some(), "missing harness extension");
        let harness = &json["harness"];
        assert!(harness.get("turns").is_some());
        assert!(harness.get("totals").is_some());
    }

    #[test]
    fn gate_counts_mirrored_into_agent_metrics() {
        let mut telem = Telemetry::new("sess-test-002", "agent-oracle");
        let m = TurnMetrics {
            gates_passed: 3,
            gates_failed: 1,
            ..TurnMetrics::default()
        };
        telem.record_turn(m);
        assert_eq!(telem.agent.gates_passed, 3);
        assert_eq!(telem.agent.gates_failed, 1);
    }

    // ── JSONL append — record shape + multi-append ────────────────────────────

    #[test]
    fn finish_appends_valid_jsonl() {
        let tmp = TempDir::new().unwrap();
        let sink = tmp.path().join("session-metrics.jsonl");

        let mut telem = Telemetry::new("sess-test-003", "agent-oracle");
        telem.record_turn(TurnMetrics {
            turn: 1,
            tokens_in: 80,
            tokens_out: 40,
            tool_calls: 1,
            denials: 0,
            gates_passed: 1,
            gates_failed: 0,
            latency_ms: 100,
            context_tokens: 200,
            token_pressure_switch: 0,
        });
        telem.finish(&sink).unwrap();

        // File must exist and contain exactly one non-empty line.
        let contents = std::fs::read_to_string(&sink).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1, "expected exactly one JSONL record");

        // The line must parse as valid JSON with the required keys.
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["sessionId"], "sess-test-003");
        assert_eq!(parsed["agentId"], "agent-oracle");
        assert!(parsed["harness"]["turns"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn finish_appends_without_truncating_existing() {
        let tmp = TempDir::new().unwrap();
        let sink = tmp.path().join("session-metrics.jsonl");

        // First session.
        let t1 = Telemetry::new("sess-001", "agent-oracle");
        t1.finish(&sink).unwrap();

        // Second session — must append, not overwrite.
        let t2 = Telemetry::new("sess-002", "agent-oracle");
        t2.finish(&sink).unwrap();

        let contents = std::fs::read_to_string(&sink).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "expected two JSONL records after two sessions"
        );

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(first["sessionId"], "sess-001");
        assert_eq!(second["sessionId"], "sess-002");
    }

    // ── Secrets must NOT appear in telemetry ──────────────────────────────────

    #[test]
    fn telemetry_record_contains_no_string_payloads_from_tools() {
        // This test documents the invariant: TurnMetrics carries only numbers.
        // We verify that the serialized form of a TurnMetrics has no string
        // values that could accidentally contain a credential.
        let m = TurnMetrics {
            turn: 1,
            tokens_in: 100,
            tokens_out: 50,
            tool_calls: 1,
            denials: 0,
            gates_passed: 1,
            gates_failed: 0,
            latency_ms: 200,
            context_tokens: 400,
            token_pressure_switch: 0,
        };
        let json = serde_json::to_value(&m).unwrap();
        // Every value in the TurnMetrics JSON must be a number, not a string.
        if let serde_json::Value::Object(map) = &json {
            for (key, val) in map {
                assert!(
                    val.is_number(),
                    "TurnMetrics field `{key}` is not a number — possible secret leak surface"
                );
            }
        }
    }

    // ── utc_now produces valid ISO 8601 format ────────────────────────────────

    #[test]
    fn utc_now_format() {
        let ts = utc_now();
        // YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "timestamp must end with Z: {ts}");
        assert_eq!(ts.len(), 20, "timestamp must be 20 chars: {ts}");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }
}
