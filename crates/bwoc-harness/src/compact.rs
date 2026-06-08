//! Context compaction for the interactive `--chat` session.
//!
//! A long chat eventually exceeds the model's context window. Rather than fail
//! (or silently truncate and lose early decisions), this module **summarizes the
//! oldest turns** into one compact note and keeps the recent tail verbatim — so
//! the agent retains the gist of the whole conversation while staying under
//! budget. Adapted from openclaude's `autoCompact`.
//!
//! Split of concerns:
//! - [`estimate_tokens`] / [`plan_compaction`] are **pure** (cheap heuristic,
//!   unit-tested) — they decide *whether* and *where* to compact.
//! - [`maybe_compact`] does the async work: summarize via the provider, splice
//!   the history, and report how many messages were folded. The caller emits the
//!   `chat_proto` notice.
//!
//! The heuristic token estimate is `chars / 4` (the usual rough rule); we never
//! need exactness — only "are we near the ceiling". The provider's real usage
//! counts still drive the per-turn display.

use crate::provider::{ChatMessage, ProviderClient, Role};

/// Rough chars-per-token divisor for the size heuristic.
const CHARS_PER_TOKEN: usize = 4;

/// Estimate the token footprint of a message slice (heuristic, not exact):
/// content length plus any tool-call argument length, divided by
/// [`CHARS_PER_TOKEN`], with a small per-message overhead for role/framing.
pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|m| {
            let content = m.content.as_deref().map(str::len).unwrap_or(0);
            let tools = m
                .tool_calls
                .as_deref()
                .map(|tcs| {
                    tcs.iter()
                        .map(|t| t.function.name.len() + t.function.arguments.len())
                        .sum::<usize>()
                })
                .unwrap_or(0);
            (content + tools) / CHARS_PER_TOKEN + 4
        })
        .sum()
}

/// Decide whether to compact and where to split.
///
/// Returns `Some(split)` when the history is over `max_tokens`: messages
/// `1..split` (everything after the system prompt at `0`, up to `split`) should
/// be summarized, and `split..` kept verbatim. Returns `None` when no compaction
/// is needed or possible.
///
/// The tail is grown from the end until it reaches ~half of `max_tokens`, so a
/// generous recent window survives. The split is then advanced past any leading
/// `tool` results so the kept tail never starts with an orphan tool message (an
/// OpenAI-compatible provider requires a `tool` message to follow an assistant
/// `tool_calls`). Compaction is skipped if it would fold fewer than two
/// messages (not worth a summarization round-trip).
pub fn plan_compaction(messages: &[ChatMessage], max_tokens: usize) -> Option<usize> {
    if max_tokens == 0 || estimate_tokens(messages) <= max_tokens {
        return None;
    }
    // Need a system prompt at [0] plus enough history to bother.
    if messages.len() < 4 {
        return None;
    }

    // Grow the tail from the end until it reaches ~half the budget. The last
    // message is always kept (even if it alone exceeds the half-budget) so the
    // tail is never empty; earlier messages join only while under budget.
    let target_tail = max_tokens / 2;
    let mut tail = 0usize;
    let mut split = messages.len();
    while split > 1 {
        let cost = estimate_tokens(std::slice::from_ref(&messages[split - 1]));
        let is_last = split == messages.len();
        if !is_last && tail + cost > target_tail {
            break;
        }
        tail += cost;
        split -= 1;
    }

    // Don't start the kept tail on an orphan tool result.
    while split < messages.len() && messages[split].role == Role::Tool {
        split += 1;
    }

    // Fold at least two messages (indices 1..split), and keep a non-empty tail.
    if split < 3 || split >= messages.len() {
        return None;
    }
    Some(split)
}

/// Render a message slice as plain text for the summarizer prompt.
fn render(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        let role = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        if let Some(c) = &m.content {
            if !c.is_empty() {
                out.push_str(role);
                out.push_str(": ");
                out.push_str(c);
                out.push('\n');
            }
        }
        if let Some(tcs) = &m.tool_calls {
            for t in tcs {
                out.push_str(&format!(
                    "assistant: [tool call] {}({})\n",
                    t.function.name, t.function.arguments
                ));
            }
        }
    }
    out
}

/// System prompt for the summarization pass.
const SUMMARIZE_SYSTEM: &str = "\
You compress a coding-assistant conversation. Summarize the excerpt below into a
concise note (under ~200 words) that preserves: decisions made, facts learned,
files created or edited, commands run, and any unfinished task or open question.
Write in compact prose or bullets. Output ONLY the summary — no preamble.";

/// Outcome of one [`compact_context`] pass — the unified engine's report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compaction {
    /// Under budget (or nothing worth folding) — history untouched.
    None,
    /// Folded `removed` messages into an LLM summary note.
    Summarized { removed: usize },
    /// Summarizer unavailable — folded `removed` messages behind a plain
    /// truncation marker (the v1 batch strategy, now the shared fallback).
    Truncated { removed: usize },
}

impl Compaction {
    /// Messages folded by this pass (0 for `None`).
    pub fn removed(&self) -> usize {
        match self {
            Compaction::None => 0,
            Compaction::Summarized { removed } | Compaction::Truncated { removed } => *removed,
        }
    }
}

/// Unified context engine (HV3-2): compact `history` in place when it is over
/// `max_tokens`. One policy for both the batch loop and `--chat`:
///
/// 1. **Summarize-first** — messages `1..split` are folded into one `system`
///    note via a single provider call (decisions, files, commands, open
///    questions survive in prose).
/// 2. **Truncate fallback** — if the summarizer fails or returns nothing, the
///    same span is folded behind a plain marker instead, so an over-long
///    history is *always* brought back under budget (no new failure mode —
///    this was the batch loop's v1 strategy).
/// 3. **Tier 2 synergy** — what falls out of the window falls into memory:
///    when the workdir's manifest configures `deepMemoryCmd`, the folded
///    content (the summary, or the raw excerpt on fallback) is written to
///    `.bwoc/compacted-context.md` and mined (`--mode compaction`), so
///    `memory_search` can still recall it later. Best-effort, like all of
///    Tier 2.
pub async fn compact_context(
    provider: &dyn ProviderClient,
    model: &str,
    max_tokens: usize,
    history: &mut Vec<ChatMessage>,
    workdir: &std::path::Path,
) -> Compaction {
    let Some(split) = plan_compaction(history, max_tokens) else {
        return Compaction::None;
    };

    let excerpt = render(&history[1..split]);
    let removed = split - 1;

    // Max-taint of the folded window (gate t3): the summary inherits untrusted
    // taint iff ANY folded message carried it, by exactly the rule the per-turn
    // scan applies (`taints_turn` — tool outputs count, the agent's own turn
    // does not). This stops compaction from laundering an Untrusted window into
    // the agent's own trust: the resulting note is a tainted `Principal::Summary`,
    // not a Trusted `SelfAgent`.
    let folded_untrusted = history[1..split]
        .iter()
        .any(|m| crate::session_trust::taints_turn(m.principal()));

    let messages = vec![
        ChatMessage::system(SUMMARIZE_SYSTEM),
        ChatMessage::user(format!("Conversation excerpt to summarize:\n\n{excerpt}")),
    ];
    let summary = match provider.complete(messages, Vec::new(), model).await {
        Ok(c) => c
            .choices
            .into_iter()
            .next()
            .and_then(|ch| ch.message.content)
            .unwrap_or_default(),
        Err(_) => String::new(),
    };

    let (note, outcome, mined_text) = if summary.trim().is_empty() {
        // Truncation fallback: kept as a plain `user` note (Unknown → Untrusted,
        // fail-closed). It is always-tainting regardless of the folded window —
        // a conservative no-op next to the summary path's precise max-taint.
        (
            ChatMessage::user(format!(
                "[context compacted: {removed} messages truncated to fit context window]"
            )),
            Compaction::Truncated { removed },
            excerpt,
        )
    } else {
        let trimmed = summary.trim().to_string();
        (
            // Folded as a `Principal::Summary` note carrying the window's
            // max-taint (gate t3) — NOT a Trusted `SelfAgent` system message.
            ChatMessage::summary(
                format!(
                    "[Summary of {removed} earlier messages — the conversation continues below]\n{trimmed}"
                ),
                folded_untrusted,
            ),
            Compaction::Summarized { removed },
            trimmed,
        )
    };

    let mut compacted = Vec::with_capacity(history.len() - removed + 1);
    compacted.push(history[0].clone()); // original system prompt
    compacted.push(note);
    compacted.extend_from_slice(&history[split..]);
    *history = compacted;

    // Tier 2 synergy — best-effort, opt-in via the manifest.
    if let Some(dm) = crate::deep_memory::DeepMemoryCmd::from_workdir(workdir) {
        let bwoc_dir = workdir.join(".bwoc");
        let artifact = bwoc_dir.join("compacted-context.md");
        let _ = std::fs::create_dir_all(&bwoc_dir);
        if std::fs::write(&artifact, &mined_text).is_ok() {
            dm.mine(&artifact, "compaction").await;
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(role_user: bool, n: usize) -> ChatMessage {
        let body = "x".repeat(n);
        if role_user {
            ChatMessage::user(body)
        } else {
            ChatMessage::assistant(Some(body), None)
        }
    }

    #[test]
    fn estimate_scales_with_length() {
        let small = estimate_tokens(&[ChatMessage::user("hi")]);
        let large = estimate_tokens(&[big(true, 4000)]);
        assert!(large > small);
        // ~4000 chars / 4 ≈ 1000 tokens (+overhead).
        assert!((1000..1100).contains(&large), "got {large}");
    }

    #[test]
    fn no_compaction_under_budget() {
        let h = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::assistant(Some("hello".into()), None),
        ];
        assert_eq!(plan_compaction(&h, 8000), None);
    }

    #[test]
    fn no_compaction_when_too_few_messages() {
        let h = vec![ChatMessage::system("sys"), big(true, 100_000)];
        assert_eq!(plan_compaction(&h, 10), None);
    }

    #[test]
    fn plans_split_when_over_budget() {
        // system + 6 large turns; small budget forces a split that keeps a tail.
        let mut h = vec![ChatMessage::system("sys")];
        for i in 0..6 {
            h.push(big(i % 2 == 0, 2000));
        }
        let split = plan_compaction(&h, 1000).expect("should compact");
        assert!(split >= 3, "fold at least two messages, got split={split}");
        assert!(split < h.len(), "must keep a non-empty tail");
    }

    #[test]
    fn split_does_not_start_tail_on_orphan_tool_result() {
        // Build: sys, user, assistant(tool_calls), tool, user, assistant ...
        let mut h = vec![ChatMessage::system("sys")];
        h.push(big(true, 3000));
        h.push(ChatMessage::assistant(Some("call".into()), None));
        h.push(ChatMessage::tool_result(
            "call-1",
            "read_file",
            "x".repeat(3000),
        ));
        h.push(big(true, 3000));
        h.push(big(false, 3000));
        if let Some(split) = plan_compaction(&h, 1500) {
            assert_ne!(
                h[split].role,
                Role::Tool,
                "tail must not begin with a tool result"
            );
        }
    }

    // ── Unified engine tests (HV3-2) ─────────────────────────────────────────

    use crate::error::HarnessError;
    use crate::provider::{ChatCompletion, Choice, FinishReason, StreamChunk, Tool, Usage};
    use async_trait::async_trait;
    use futures_util::Stream;
    use std::pin::Pin;
    use std::sync::Mutex;

    /// Mock provider: pops queued results; `stream` is never used by the engine.
    struct EngineMock {
        responses: Mutex<Vec<Result<ChatCompletion, HarnessError>>>,
    }

    impl EngineMock {
        fn summarizing(text: &str) -> Self {
            Self {
                responses: Mutex::new(vec![Ok(ChatCompletion {
                    id: "mock".to_string(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChatMessage::assistant(Some(text.to_string()), None),
                        finish_reason: Some(FinishReason::Stop),
                    }],
                    usage: Some(Usage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    }),
                })]),
            }
        }

        fn failing() -> Self {
            Self {
                responses: Mutex::new(vec![Err(HarnessError::Provider("down".to_string()))]),
            }
        }
    }

    #[async_trait]
    impl ProviderClient for EngineMock {
        async fn complete(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<Tool>,
            _model: &str,
        ) -> Result<ChatCompletion, HarnessError> {
            let mut lock = self.responses.lock().unwrap();
            if lock.is_empty() {
                return Err(HarnessError::Provider("mock exhausted".to_string()));
            }
            lock.remove(0)
        }

        async fn stream(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<Tool>,
            _model: &str,
        ) -> Result<
            Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>,
            HarnessError,
        > {
            Err(HarnessError::Provider("stream unused".to_string()))
        }

        async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
            Ok(())
        }
    }

    /// system + n chunky user messages — comfortably over a small budget.
    fn over_budget_history(n: usize) -> Vec<ChatMessage> {
        let mut h = vec![ChatMessage::system("sys prompt")];
        for i in 0..n {
            h.push(ChatMessage::user(format!(
                "message {i} {}",
                "x".repeat(120)
            )));
        }
        h
    }

    #[tokio::test]
    async fn engine_under_budget_is_none() {
        let mock = EngineMock::summarizing("unused");
        let mut h = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::user("there"),
            ChatMessage::user("ok"),
        ];
        let before = h.len();
        let out = compact_context(&mock, "m", 100_000, &mut h, std::path::Path::new("/tmp")).await;
        assert_eq!(out, Compaction::None);
        assert_eq!(h.len(), before, "history untouched under budget");
    }

    #[tokio::test]
    async fn engine_summarizes_when_provider_succeeds() {
        let mock = EngineMock::summarizing("DECISIONS: kept rustls; edited foo.rs");
        let mut h = over_budget_history(12);
        let out = compact_context(&mock, "m", 60, &mut h, std::path::Path::new("/tmp")).await;
        let removed = match out {
            Compaction::Summarized { removed } => removed,
            other => panic!("expected Summarized, got {other:?}"),
        };
        assert!(removed >= 2);
        assert_eq!(h[0].content.as_deref(), Some("sys prompt"));
        let note = h[1].content.as_deref().unwrap_or("");
        assert!(note.contains("Summary of"), "got note: {note}");
        assert!(note.contains("kept rustls"));
    }

    #[tokio::test]
    async fn engine_falls_back_to_truncation_on_summarizer_failure() {
        let mock = EngineMock::failing();
        let mut h = over_budget_history(12);
        let before = h.len();
        let out = compact_context(&mock, "m", 60, &mut h, std::path::Path::new("/tmp")).await;
        let removed = match out {
            Compaction::Truncated { removed } => removed,
            other => panic!("expected Truncated fallback, got {other:?}"),
        };
        assert!(removed >= 2);
        assert!(h.len() < before, "fallback must still shrink the history");
        let note = h[1].content.as_deref().unwrap_or("");
        assert!(note.contains("context compacted"), "got note: {note}");
    }

    /// GATE t3 — message-level taint propagation through compaction. Was
    /// `#[ignore]`d at t2; now PASSES by real implementation (condition 4).
    ///
    /// When the unified context engine summarizes a window that contained
    /// Untrusted content (a tool result, a teammate message), the note is now
    /// built with [`ChatMessage::summary`] carrying the window's **max-taint** →
    /// [`Principal::Summary { tainted: true }`](bwoc_core::trust::Principal::Summary)
    /// → **Untrusted**. The pre-t3 code built it with `ChatMessage::system` →
    /// `SelfAgent` → Trusted, laundering untrusted content into the agent's own
    /// highest trust at the compaction boundary; that hole is now closed.
    ///
    /// This is defense-in-depth that complements — does not replace — the
    /// monotonic session-trust latch
    /// ([`crate::session_trust::SessionTrust`]), which independently holds the
    /// turn Untrusted across compaction and reload (see
    /// `gate_survives_compaction_no_effectful_reopen` below). The test kept its
    /// `gate_t2` name from when it tracked the still-open hole (no relabel — no
    /// musavada); the behavior it asserts is delivered by gate t3.
    #[tokio::test]
    async fn compaction_summary_inherits_untrusted_taint_gate_t2() {
        use bwoc_core::trust::TrustLevel;

        let mock = EngineMock::summarizing("a tidy summary of the secrets");
        // The folded region (early messages) unambiguously contains Untrusted
        // content — a teammate message and a tool result. The tail is plain user
        // messages (no trailing tool result) so a summary is actually produced.
        let mut h = vec![ChatMessage::system("sys prompt")];
        h.push(ChatMessage::ingest(
            bwoc_core::trust::Principal::TeamPeer {
                agent: "peer".into(),
            },
            format!("peer leaked {}", "x".repeat(120)),
        ));
        h.push(ChatMessage::tool_result(
            "c1",
            "read_file",
            format!("secret {}", "x".repeat(120)),
        ));
        for i in 0..10 {
            h.push(ChatMessage::user(format!(
                "message {i} {}",
                "x".repeat(120)
            )));
        }

        let out = compact_context(&mock, "m", 60, &mut h, std::path::Path::new("/tmp")).await;
        assert!(
            matches!(out, Compaction::Summarized { .. }),
            "precondition: the untrusted teammate/tool messages must be folded; got {out:?}"
        );

        let summary = &h[1];
        assert_eq!(
            summary.role,
            Role::System,
            "summary is folded as a system note"
        );
        // The taint that t2 must enforce, and that t1 deliberately does NOT:
        assert_eq!(
            summary.trust(),
            TrustLevel::Untrusted,
            "a summary folding Untrusted content must itself be Untrusted (gate t2)"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_mines_folded_content_when_tier2_configured() {
        // Workdir with a manifest whose deepMemoryCmd is `true` (exit 0, no
        // output) — asserts the synergy path writes the artifact and doesn't
        // disturb the outcome.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.manifest.json"),
            r#"{"name":"t","agentId":"agent-t","agentRole":"r","primaryModel":"m",
                "memoryPath":"memories/","lintCmd":"true","formatCmd":"true",
                "testCmd":"true","buildCmd":"true","deepMemoryCmd":"true",
                "version":"1"}"#,
        )
        .unwrap();
        let mock = EngineMock::summarizing("THE SUMMARY");
        let mut h = over_budget_history(12);
        let out = compact_context(&mock, "m", 60, &mut h, dir.path()).await;
        assert!(matches!(out, Compaction::Summarized { .. }));
        let artifact = dir.path().join(".bwoc/compacted-context.md");
        assert!(artifact.is_file(), "folded content must be persisted");
        let body = std::fs::read_to_string(artifact).unwrap();
        assert!(body.contains("THE SUMMARY"));
    }

    /// GATE t3 — defense in depth survives compaction (C1 + message-level taint).
    ///
    /// Two independent protections now hold the turn Untrusted after compaction
    /// folds an Untrusted window:
    /// 1. The compaction note inherits the window's max-taint
    ///    ([`Principal::Summary { tainted: true }`](bwoc_core::trust::Principal::Summary)),
    ///    so even a *fresh scan* of the post-compaction history reads Untrusted
    ///    (the t3 fix — pre-t3 it laundered to a Trusted `SelfAgent` and scanned
    ///    Trusted).
    /// 2. The monotonic sticky latch, set on the pre-compaction turn, holds the
    ///    turn Untrusted regardless of any scan.
    /// Either alone refuses the effectful tool; this asserts both. It PASSES —
    /// that is the point.
    #[tokio::test]
    async fn gate_survives_compaction_no_effectful_reopen() {
        use crate::policy::{Mode, Policy, PolicyOutcome, run_pipeline};
        use crate::session_trust::{SessionTrust, scan_turn_trust};
        use bwoc_core::trust::{Principal, TrustLevel};

        let allow_all = Policy {
            default_mode: Mode::Allow,
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
        };
        let wt = std::path::Path::new("/tmp/agent-luban/gate-survive");

        // Build an over-budget history whose folded window carries Untrusted
        // ingress (a teammate message), with a plain user tail so a summary is
        // produced.
        let mut h = vec![ChatMessage::system("sys prompt")];
        h.push(ChatMessage::ingest(
            Principal::TeamPeer {
                agent: "peer".into(),
            },
            format!("peer says {}", "x".repeat(120)),
        ));
        // The tail is operator() (Trusted). Pre-t3 a fresh post-compaction scan
        // read Trusted here (the laundering); at t3 the folded teammate taints
        // the summary, so the scan reads Untrusted — demonstrated below.
        for i in 0..10 {
            h.push(ChatMessage::operator(format!(
                "message {i} {}",
                "x".repeat(120)
            )));
        }

        // Pre-compaction: the latch observes the Untrusted teammate ingress.
        let mut trust = SessionTrust::default();
        assert_eq!(trust.observe(&h), TrustLevel::Untrusted);
        assert!(trust.latched());

        // Compact. The Untrusted teammate message is folded into a Trusted
        // SelfAgent system summary.
        let mock = EngineMock::summarizing("a tidy summary");
        let out = compact_context(&mock, "m", 60, &mut h, std::path::Path::new("/tmp")).await;
        assert!(
            matches!(out, Compaction::Summarized { .. }),
            "precondition: the untrusted window must be folded; got {out:?}"
        );
        // t3: the summary inherited the folded teammate's taint, so a fresh scan
        // now reads Untrusted — the laundering the pre-t3 code allowed is closed.
        assert_eq!(
            scan_turn_trust(&h),
            TrustLevel::Untrusted,
            "t3: compaction must propagate taint — the summary is tainted Untrusted"
        );
        // And the summary note itself is a tainted Summary, not a Trusted SelfAgent.
        assert!(
            matches!(
                h[1].principal(),
                bwoc_core::trust::Principal::Summary { tainted: true }
            ),
            "the folded note must be a tainted Summary; got {:?}",
            h[1].principal()
        );

        // The latch is independently sticky: post-compaction turn_trust stays Untrusted…
        assert_eq!(
            trust.observe(&h),
            TrustLevel::Untrusted,
            "latch must not clear"
        );
        // …so the capability gate still refuses an effectful tool.
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "echo hi"}"#,
            wt,
            &allow_all,
            false,
            trust.turn_trust(),
        );
        assert!(
            matches!(outcome, PolicyOutcome::CapabilityDenied { .. }),
            "compaction must NOT re-open effectful tools; got {outcome:?}"
        );
    }
}
