//! `primaryModel: "auto"` resolution.
//!
//! When an agent's manifest sets `primaryModel: "auto"`, the harness does not
//! receive a concrete model on the CLI — it receives the literal `"auto"` and
//! a candidate pool (`autoModels`). This module turns that pool into a single
//! chosen model by applying four criteria, in order, against the **live**
//! provider:
//!
//! 1. **Availability** — keep only candidates the provider currently serves
//!    (`ProviderClient::list_models`). If the provider can't be probed (empty
//!    list), availability is treated as *unknown* and the filter is skipped
//!    rather than rejecting everything.
//! 2. **Capability / context fit** — probe each survivor's context window
//!    (`ProviderClient::model_context_limit`) and keep those large enough for
//!    the task's estimated token need. Candidates with an *unknown* limit are
//!    kept (can't disqualify on missing data); if every candidate's known
//!    limit is too small, the largest one is chosen as a best-effort fallback.
//! 3. **Task class** — a keyword/length heuristic splits work into `Heavy`
//!    (reasoning- or context-heavy → prefer the most capable / largest-context
//!    model) and `Light` (mechanical → prefer the cheapest).
//! 4. **Cost** — the candidate pool's order *is* the cost axis by convention
//!    (first = most capable/expensive, last = cheapest). `Light` tasks pick the
//!    cheapest fitting candidate; `Heavy` ties break toward earliest preference.
//!
//! Resolution is fully deterministic (no clock, no RNG) so a given
//! `(candidates, provider state, task)` always yields the same choice — which
//! also makes it unit-testable with a mock provider.

use std::collections::HashMap;

use crate::error::HarnessError;
use crate::provider::ProviderClient;

/// The sentinel value of `primaryModel` that triggers auto-resolution.
pub const AUTO_SENTINEL: &str = "auto";

/// Outcome of auto-resolution.
///
/// Besides the `chosen` model, this carries by-products the harness wires into
/// `LoopConfig` so the otherwise-dormant fallback / context / token-pressure
/// fields get populated from real provider data instead of being left empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoSelection {
    /// The model the loop will run with.
    pub chosen: String,
    /// Available candidates other than `chosen`, in preference order. Feeds
    /// both the error-based `fallback_models` chain and the proactive
    /// `token_pressure_models` switch list.
    pub remaining: Vec<String>,
    /// Probed context-window limits for the available candidates. Absent /
    /// unknown limits are omitted (never stored as `0`). Feeds
    /// `LoopConfig::model_context_limits`.
    pub context_limits: HashMap<String, u32>,
}

/// Heuristic task class — see module docs, criterion 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClass {
    /// Reasoning- or context-heavy: prefer the largest-context candidate.
    Heavy,
    /// Light / mechanical: prefer the cheapest fitting candidate.
    Light,
}

/// Keywords (EN + TH) that mark a task as reasoning-/context-heavy.
const HEAVY_KEYWORDS: &[&str] = &[
    "refactor",
    "architect",
    "design",
    "analyze",
    "analyse",
    "debug",
    "investigate",
    "audit",
    "migrate",
    "prove",
    "reason",
    "plan",
    "optimize",
    "optimise",
    "ออกแบบ",
    "วิเคราะห์",
    "รีแฟคเตอร์",
    "ดีบัก",
    "ตรวจสอบ",
    "วางแผน",
];

/// Above this estimated token need a task is `Heavy` regardless of keywords —
/// a large prompt alone signals context-heavy work.
const HEAVY_TOKEN_THRESHOLD: u32 = 8_192;

/// Rough token estimate for a task: ~4 chars/token plus fixed headroom for the
/// system prompt, tool schemas, and the model's own output. Deliberately
/// conservative — over-estimating biases toward a larger-context model, which
/// is the safe direction.
pub fn estimate_task_tokens(task: &str) -> u32 {
    const HEADROOM: u32 = 4_096;
    let body = u32::try_from(task.chars().count() / 4).unwrap_or(u32::MAX);
    body.saturating_add(HEADROOM)
}

/// Classify a task by keyword + length heuristic.
pub fn classify_task(task: &str) -> TaskClass {
    if estimate_task_tokens(task) >= HEAVY_TOKEN_THRESHOLD {
        return TaskClass::Heavy;
    }
    let lower = task.to_lowercase();
    if HEAVY_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        TaskClass::Heavy
    } else {
        TaskClass::Light
    }
}

/// Resolve `primaryModel: "auto"` to a concrete model.
///
/// `candidates` is the operator's `autoModels` pool in preference order.
/// Returns [`HarnessError::NoAutoCandidate`] when the pool is empty or nothing
/// survives the availability filter.
pub async fn resolve_auto(
    provider: &dyn ProviderClient,
    candidates: &[String],
    task: &str,
) -> Result<AutoSelection, HarnessError> {
    if candidates.is_empty() {
        return Err(HarnessError::NoAutoCandidate {
            reason: "autoModels is empty — `primaryModel: \"auto\"` needs a candidate pool".into(),
            candidates: Vec::new(),
        });
    }

    // --- Criterion 1: availability -----------------------------------------
    // Empty provider list ≡ "unknown" → skip the filter (keep all candidates).
    let served = provider.list_models().await;
    let available: Vec<String> = if served.is_empty() {
        candidates.to_vec()
    } else {
        candidates
            .iter()
            .filter(|c| served.iter().any(|s| s == *c))
            .cloned()
            .collect()
    };
    if available.is_empty() {
        return Err(HarnessError::NoAutoCandidate {
            reason: "none of the candidates are served by the provider".into(),
            candidates: candidates.to_vec(),
        });
    }

    // --- Probe context limits (best-effort) for the available set ----------
    let mut context_limits: HashMap<String, u32> = HashMap::new();
    for c in &available {
        if let Some(limit) = provider.model_context_limit(c).await {
            if limit > 0 {
                context_limits.insert(c.clone(), limit);
            }
        }
    }

    // --- Criterion 2: context fit ------------------------------------------
    // Keep candidates whose limit is unknown OR large enough. A model with a
    // *known* too-small window is the only kind we drop here.
    let needed = estimate_task_tokens(task);
    let fitting: Vec<String> = available
        .iter()
        .filter(|c| context_limits.get(*c).is_none_or(|&l| l >= needed))
        .cloned()
        .collect();

    // If every candidate has a known-too-small window, fall back to the one
    // with the largest known window rather than failing the run.
    let pool: Vec<String> = if fitting.is_empty() {
        let largest = available
            .iter()
            .max_by_key(|c| context_limits.get(*c).copied().unwrap_or(0))
            .cloned()
            .expect("available is non-empty");
        vec![largest]
    } else {
        fitting
    };

    // --- Criteria 3 + 4: task class picks within the fitting pool ----------
    let chosen = match classify_task(task) {
        // Heavy → most capable = largest known context window; tie-break to the
        // earliest (most preferred) candidate. Unknown windows sort as 0, so a
        // candidate with a known large window wins over an unknown one.
        TaskClass::Heavy => pool
            .iter()
            .enumerate()
            .max_by_key(|(idx, c)| {
                let limit = context_limits.get(*c).copied().unwrap_or(0);
                // Higher limit wins; on a tie, lower index (earlier) wins, so
                // negate the index via reverse ordering.
                (limit, std::cmp::Reverse(*idx))
            })
            .map(|(_, c)| c.clone())
            .expect("pool is non-empty"),
        // Light → cheapest fitting candidate = last in preference order.
        TaskClass::Light => pool.last().cloned().expect("pool is non-empty"),
    };

    let remaining: Vec<String> = available
        .iter()
        .filter(|c| **c != chosen)
        .cloned()
        .collect();

    Ok(AutoSelection {
        chosen,
        remaining,
        context_limits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::pin::Pin;

    use async_trait::async_trait;
    use futures_util::Stream;

    use crate::provider::{ChatCompletion, ChatMessage, StreamChunk, Tool};

    /// Minimal provider mock: only `list_models` + `model_context_limit` are
    /// exercised by the resolver; completion paths are never called.
    struct FakeProvider {
        served: Vec<String>,
        limits: HashMap<String, u32>,
    }

    impl FakeProvider {
        fn new(served: &[&str]) -> Self {
            Self {
                served: served.iter().map(|s| s.to_string()).collect(),
                limits: HashMap::new(),
            }
        }
        fn with_limit(mut self, model: &str, limit: u32) -> Self {
            self.limits.insert(model.to_string(), limit);
            self
        }
    }

    #[async_trait]
    impl ProviderClient for FakeProvider {
        async fn complete(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<Tool>,
            _model: &str,
        ) -> Result<ChatCompletion, HarnessError> {
            unreachable!("resolver never calls complete")
        }
        async fn stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<Tool>,
            _model: &str,
        ) -> Result<
            Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>,
            HarnessError,
        > {
            unreachable!("resolver never calls stream")
        }
        async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
            Ok(())
        }
        async fn list_models(&self) -> Vec<String> {
            self.served.clone()
        }
        async fn model_context_limit(&self, model: &str) -> Option<u32> {
            self.limits.get(model).copied()
        }
    }

    fn cands(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn empty_pool_errors() {
        let p = FakeProvider::new(&["a"]);
        let err = resolve_auto(&p, &[], "do a thing").await.unwrap_err();
        assert!(matches!(err, HarnessError::NoAutoCandidate { .. }));
    }

    #[tokio::test]
    async fn none_available_errors() {
        let p = FakeProvider::new(&["x", "y"]);
        let err = resolve_auto(&p, &cands(&["a", "b"]), "task")
            .await
            .unwrap_err();
        assert!(matches!(err, HarnessError::NoAutoCandidate { .. }));
    }

    #[tokio::test]
    async fn availability_filters_pool() {
        // "big" is in autoModels but not served → must not be chosen, and must
        // not appear in `remaining` either.
        let p = FakeProvider::new(&["small"]);
        let sel = resolve_auto(&p, &cands(&["big", "small"]), "tidy up")
            .await
            .unwrap();
        assert_eq!(sel.chosen, "small");
        assert!(sel.remaining.is_empty());
    }

    #[tokio::test]
    async fn light_task_picks_cheapest_last() {
        // Both served, both fit; Light task → cheapest = last in preference.
        let p = FakeProvider::new(&["big", "small"])
            .with_limit("big", 128_000)
            .with_limit("small", 32_000);
        let sel = resolve_auto(&p, &cands(&["big", "small"]), "fix typo")
            .await
            .unwrap();
        assert_eq!(sel.chosen, "small");
        assert_eq!(sel.remaining, vec!["big".to_string()]);
        assert_eq!(sel.context_limits.get("big"), Some(&128_000));
    }

    #[tokio::test]
    async fn heavy_task_picks_largest_context() {
        // Heavy keyword "refactor" → largest known window wins regardless of
        // preference order (here "small" is listed first).
        let p = FakeProvider::new(&["small", "big"])
            .with_limit("small", 32_000)
            .with_limit("big", 128_000);
        let sel = resolve_auto(&p, &cands(&["small", "big"]), "refactor the module")
            .await
            .unwrap();
        assert_eq!(sel.chosen, "big");
    }

    #[tokio::test]
    async fn context_fit_drops_too_small_known_window() {
        // A huge task that needs more than "small" offers; Light keyword but
        // "small" is disqualified on context, leaving "big".
        let big_task = "x".repeat(80_000); // ~20k tokens estimate
        let p = FakeProvider::new(&["small", "big"])
            .with_limit("small", 4_096)
            .with_limit("big", 128_000);
        let sel = resolve_auto(&p, &cands(&["big", "small"]), &big_task)
            .await
            .unwrap();
        assert_eq!(sel.chosen, "big");
    }

    #[tokio::test]
    async fn unknown_limit_is_not_disqualified() {
        // No probed limits at all → every candidate "fits"; Light task picks
        // the cheapest (last). Confirms unknown != disqualified.
        let p = FakeProvider::new(&["a", "b"]);
        let sel = resolve_auto(&p, &cands(&["a", "b"]), "rename var")
            .await
            .unwrap();
        assert_eq!(sel.chosen, "b");
    }

    #[tokio::test]
    async fn empty_served_skips_availability_filter() {
        // Provider can't list models (empty) → availability unknown, keep all.
        let p = FakeProvider {
            served: Vec::new(),
            limits: HashMap::new(),
        };
        let sel = resolve_auto(&p, &cands(&["a", "b"]), "light task")
            .await
            .unwrap();
        // Light → last; both retained because filter was skipped.
        assert_eq!(sel.chosen, "b");
        assert_eq!(sel.remaining, vec!["a".to_string()]);
    }

    #[test]
    fn classify_heavy_by_keyword_and_length() {
        assert_eq!(classify_task("refactor this"), TaskClass::Heavy);
        assert_eq!(classify_task("ออกแบบ schema"), TaskClass::Heavy);
        assert_eq!(classify_task("fix a typo"), TaskClass::Light);
        // Long prompt alone → Heavy.
        let long = "a".repeat(40_000);
        assert_eq!(classify_task(&long), TaskClass::Heavy);
    }
}
