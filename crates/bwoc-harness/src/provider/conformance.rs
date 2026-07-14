//! Cross-implementation conformance tests for [`ProviderClient`].
//!
//! Hermetic — every backend is a `wiremock` HTTP server or (for the CLI) a fake
//! subprocess; **no live network**. These assert that the trait *contract* holds
//! uniformly across the two implementations that speak HTTP — [`OllamaClient`]
//! (which backs `ollama`, `openrouter`, and `litellm`) and [`AnthropicClient`] —
//! so a new OpenAI-compatible or Anthropic-shaped backend cannot silently drift
//! from the behaviour the agent loop relies on.
//!
//! **Intentional non-uniformity (documented, not tested here as uniform):**
//! - `CliClient` validates the *binary*, not the model (model validity surfaces
//!   at completion time), and has no HTTP-status error mapping — its contract is
//!   covered by the subprocess tests in [`super::cli`], plus the graceful-default
//!   checks at the bottom of this file.
//! - Even between the two HTTP impls, `404` diverges: `OllamaClient` maps it to
//!   `ModelNotFound` (an OpenAI-compat convention), while `AnthropicClient` lets
//!   `classify_http_error` map it to a fatal `Provider` error. The error-mapping
//!   contract below therefore exercises `5xx` (uniformly transient) and a generic
//!   `4xx` like `401` (uniformly fatal), not `404`.

#![cfg(test)]

use futures_util::StreamExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::types::ChatMessage;
use super::{AnthropicClient, OllamaClient, ProviderClient};
use crate::error::HarnessError;

fn user() -> Vec<ChatMessage> {
    vec![ChatMessage::user("hi")]
}

/// One HTTP `ProviderClient` implementation plus the knowledge of how to program
/// its mock backend for each scenario. The concrete request/response *shapes*
/// differ per vendor; the trait output they must produce does not.
#[async_trait::async_trait]
trait HttpSubject {
    const NAME: &'static str;
    const MODEL: &'static str;

    /// A client pointed at the mock server `uri`.
    fn client(uri: &str) -> Box<dyn ProviderClient>;

    /// Mount `GET …/models` returning the OpenAI-compat list shape for `ids`.
    async fn mount_models(server: &MockServer, ids: &[&str]);
    /// Mount a successful completion whose assistant text is `hello`.
    async fn mount_completion_ok(server: &MockServer);
    /// Mount a completion that returns HTTP `status` with a short error body.
    async fn mount_completion_status(server: &MockServer, status: u16);
    /// Mount a 200 completion with a body that is not valid JSON.
    async fn mount_completion_malformed(server: &MockServer);
    /// Mount a successful SSE stream whose deltas reassemble to `hello`.
    async fn mount_stream_ok(server: &MockServer);
}

// ── Subject: OllamaClient (OpenAI-compatible; backs ollama/openrouter/litellm) ─

struct Ollama;

#[async_trait::async_trait]
impl HttpSubject for Ollama {
    const NAME: &'static str = "ollama";
    const MODEL: &'static str = "m1";

    fn client(uri: &str) -> Box<dyn ProviderClient> {
        Box::new(OllamaClient::new(format!("{uri}/v1")))
    }

    async fn mount_models(server: &MockServer, ids: &[&str]) {
        let data: Vec<_> = ids.iter().map(|id| serde_json::json!({"id": id})).collect();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": data})),
            )
            .mount(server)
            .await;
    }

    async fn mount_completion_ok(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "c1",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hello"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(server)
            .await;
    }

    async fn mount_completion_status(server: &MockServer, status: u16) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(status).set_body_string("upstream error"))
            .mount(server)
            .await;
    }

    async fn mount_completion_malformed(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json {"))
            .mount(server)
            .await;
    }

    async fn mount_stream_ok(server: &MockServer) {
        let body = "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hel\"},\"finish_reason\":null}]}\n\n\
                    data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\n\
                    data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(server)
            .await;
    }
}

// ── Subject: AnthropicClient (native Messages API) ────────────────────────────

struct Anthropic;

#[async_trait::async_trait]
impl HttpSubject for Anthropic {
    const NAME: &'static str = "anthropic";
    const MODEL: &'static str = "claude-x";

    fn client(uri: &str) -> Box<dyn ProviderClient> {
        // Inject a key so `require_key` passes without mutating process env.
        Box::new(AnthropicClient::new(uri).with_api_key("test-key"))
    }

    async fn mount_models(server: &MockServer, ids: &[&str]) {
        let data: Vec<_> = ids.iter().map(|id| serde_json::json!({"id": id})).collect();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": data})),
            )
            .mount(server)
            .await;
    }

    async fn mount_completion_ok(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "m1",
                "content": [{"type": "text", "text": "hello"}],
                "stop_reason": "end_turn"
            })))
            .mount(server)
            .await;
    }

    async fn mount_completion_status(server: &MockServer, status: u16) {
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(status).set_body_string("upstream error"))
            .mount(server)
            .await;
    }

    async fn mount_completion_malformed(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json {"))
            .mount(server)
            .await;
    }

    async fn mount_stream_ok(server: &MockServer) {
        // Anthropic's SSE is a stateful event protocol; one text_delta suffices
        // to prove the translation reassembles to non-empty content.
        let body = "event: content_block_delta\n\
                    data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n";
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(server)
            .await;
    }
}

// ── The shared contract ───────────────────────────────────────────────────────

/// Reassemble a stream's text deltas into one string (the loop's own behaviour).
async fn drain_stream(
    mut s: std::pin::Pin<
        Box<
            dyn futures_util::Stream<Item = Result<super::types::StreamChunk, HarnessError>> + Send,
        >,
    >,
    who: &str,
) -> String {
    let mut text = String::new();
    while let Some(item) = s.next().await {
        let chunk = item.unwrap_or_else(|e| panic!("[{who}] stream chunk error: {e:?}"));
        if let Some(c) = chunk.choices.first() {
            if let Some(t) = &c.delta.content {
                text.push_str(t);
            }
        }
    }
    text
}

/// Run every contract assertion against one HTTP subject. Each check uses a fresh
/// `MockServer` so the mounts never collide.
async fn assert_http_contract<S: HttpSubject>() {
    let who = S::NAME;

    // 1a. validate_model — a served model resolves.
    {
        let server = MockServer::start().await;
        S::mount_models(&server, &[S::MODEL]).await;
        S::client(&server.uri())
            .validate_model(S::MODEL)
            .await
            .unwrap_or_else(|e| panic!("[{who}] validate_model(served) must be Ok, got {e:?}"));
    }
    // 1b. validate_model — an absent model is ModelNotFound.
    {
        let server = MockServer::start().await;
        S::mount_models(&server, &[]).await;
        let err = S::client(&server.uri())
            .validate_model("definitely-absent")
            .await
            .expect_err("unknown model must error");
        assert!(
            matches!(err, HarnessError::ModelNotFound(_)),
            "[{who}] absent model must be ModelNotFound, got {err:?}"
        );
    }

    // 2. complete — success yields the assistant text.
    {
        let server = MockServer::start().await;
        S::mount_completion_ok(&server).await;
        let done = S::client(&server.uri())
            .complete(user(), vec![], S::MODEL)
            .await
            .unwrap_or_else(|e| panic!("[{who}] complete must succeed, got {e:?}"));
        assert_eq!(
            done.choices[0].message.content.as_deref(),
            Some("hello"),
            "[{who}] complete content"
        );
    }

    // 3a. error mapping — 5xx is transient (retryable).
    {
        let server = MockServer::start().await;
        S::mount_completion_status(&server, 503).await;
        let err = S::client(&server.uri())
            .complete(user(), vec![], S::MODEL)
            .await
            .expect_err("5xx must error");
        assert!(
            matches!(err, HarnessError::TransientProvider(_)),
            "[{who}] 5xx must be TransientProvider, got {err:?}"
        );
    }
    // 3b. error mapping — a generic 4xx is fatal.
    {
        let server = MockServer::start().await;
        S::mount_completion_status(&server, 401).await;
        let err = S::client(&server.uri())
            .complete(user(), vec![], S::MODEL)
            .await
            .expect_err("401 must error");
        assert!(
            matches!(err, HarnessError::Provider(_)),
            "[{who}] 401 must be a fatal Provider error, got {err:?}"
        );
    }

    // 4. list_models — advertised ids are returned.
    {
        let server = MockServer::start().await;
        S::mount_models(&server, &[S::MODEL, "other-model"]).await;
        let models = S::client(&server.uri()).list_models().await;
        assert!(
            models.iter().any(|m| m == S::MODEL),
            "[{who}] list_models must include the advertised model, got {models:?}"
        );
    }

    // 5. malformed success body — a Provider (parse) error, never a panic.
    {
        let server = MockServer::start().await;
        S::mount_completion_malformed(&server).await;
        let err = S::client(&server.uri())
            .complete(user(), vec![], S::MODEL)
            .await
            .expect_err("malformed body must error");
        assert!(
            matches!(err, HarnessError::Provider(_)),
            "[{who}] malformed body must map to Provider, got {err:?}"
        );
    }

    // 6. stream — deltas reassemble to the full content and the stream ends.
    {
        let server = MockServer::start().await;
        S::mount_stream_ok(&server).await;
        let stream = S::client(&server.uri())
            .stream(user(), vec![], S::MODEL)
            .await
            .unwrap_or_else(|e| panic!("[{who}] stream must open, got {e:?}"));
        let text = drain_stream(stream, who).await;
        assert_eq!(
            text, "hello",
            "[{who}] stream must reassemble to full content"
        );
    }
}

#[tokio::test]
async fn ollama_client_satisfies_provider_contract() {
    assert_http_contract::<Ollama>().await;
}

#[tokio::test]
async fn anthropic_client_satisfies_provider_contract() {
    assert_http_contract::<Anthropic>().await;
}

// ── CliClient graceful-default contract ───────────────────────────────────────
//
// CliClient's active surface (completion parsing, unknown-model→ModelNotFound,
// nonzero-exit→Provider, single-chunk stream, binary validation) is covered by
// the subprocess tests in `super::cli`. What those do not assert is the
// best-effort *default* half of the trait, which a chat-only CLI backend cannot
// implement and must degrade gracefully: `list_models` → empty and
// `model_context_limit` → None (so `primaryModel: "auto"` resolution and the
// context-window fallback treat it as "unknown", never crash). These use a fake
// CLI that is never invoked, so no subprocess is spawned.

#[cfg(unix)]
#[tokio::test]
async fn cli_client_list_models_defaults_empty() {
    let client = super::CliClient::new("/nonexistent/fake-cli");
    assert!(
        client.list_models().await.is_empty(),
        "a chat-only CLI backend must report no listable models (auto-resolution reads this as 'unknown')"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn cli_client_context_limit_defaults_none() {
    let client = super::CliClient::new("/nonexistent/fake-cli");
    assert!(
        client.model_context_limit("any").await.is_none(),
        "a CLI backend cannot report a context window; the loop must fall back to its default"
    );
}
