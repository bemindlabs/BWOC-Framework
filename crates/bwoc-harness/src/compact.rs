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

use crate::error::HarnessResult;
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

/// Compact `history` in place if it is over `max_tokens`.
///
/// On compaction: messages `1..split` are summarized (via a single provider
/// call) into one `system` note that replaces them, the recent tail is kept, and
/// the number of folded messages is returned as `Some(removed)`. Returns
/// `Ok(None)` when no compaction was needed. A summarizer failure is **not**
/// fatal — it returns `Ok(None)` so the turn proceeds uncompacted (the provider
/// may still reject an over-long prompt, but we never crash the session here).
pub async fn maybe_compact(
    provider: &dyn ProviderClient,
    model: &str,
    max_tokens: usize,
    history: &mut Vec<ChatMessage>,
) -> HarnessResult<Option<usize>> {
    let Some(split) = plan_compaction(history, max_tokens) else {
        return Ok(None);
    };

    let excerpt = render(&history[1..split]);
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
        // Summarizer call failed — leave history untouched, proceed uncompacted.
        Err(_) => return Ok(None),
    };
    if summary.trim().is_empty() {
        return Ok(None);
    }

    let removed = split - 1;
    let note = ChatMessage::system(format!(
        "[Summary of {removed} earlier messages — the conversation continues below]\n{}",
        summary.trim()
    ));
    let mut compacted = Vec::with_capacity(history.len() - removed + 1);
    compacted.push(history[0].clone()); // original system prompt
    compacted.push(note);
    compacted.extend_from_slice(&history[split..]);
    *history = compacted;

    Ok(Some(removed))
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
        h.push(ChatMessage::tool_result("call-1", "x".repeat(3000)));
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
}
