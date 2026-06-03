//! Embedding seam — turns text into vectors.
//!
//! The seam mirrors `bwoc-core::deep_memory`'s injectable-runner pattern: the
//! [`Embedder`] trait has a real HTTP implementation ([`HttpEmbedder`], talking
//! to any OpenAI-compatible `POST /v1/embeddings` endpoint) and a deterministic
//! [`StubEmbedder`] so `cargo test` never touches the network.

use serde::Deserialize;

/// Errors raised while embedding text.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The HTTP request itself failed (connection, timeout, TLS, …).
    #[error("embedding request failed: {0}")]
    Request(String),
    /// The endpoint returned a non-success status.
    #[error("embedding endpoint returned {status}: {body}")]
    Status { status: u16, body: String },
    /// The response body did not match the expected `{ data: [{ embedding }] }`.
    #[error("malformed embedding response: {0}")]
    Decode(String),
    /// The endpoint returned a different number of vectors than inputs given.
    #[error("embedding count mismatch: asked for {asked}, got {got}")]
    CountMismatch { asked: usize, got: usize },
}

/// Anything that can turn a batch of texts into equal-length vectors.
///
/// Implementations must return one vector per input, in order. All returned
/// vectors are expected to share a dimension (the caller relies on this for
/// cosine similarity); a backend that violates it is a bug in that backend.
pub trait Embedder {
    /// Embed `texts`, returning one vector per input in the same order.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;

    /// Convenience: embed a single string.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut v = self.embed(&[text.to_string()])?;
        v.pop()
            .ok_or(EmbedError::CountMismatch { asked: 1, got: 0 })
    }
}

// ---------------------------------------------------------------------------
// HttpEmbedder — real OpenAI-compatible `/v1/embeddings` client
// ---------------------------------------------------------------------------

/// Calls `POST {base_url}/v1/embeddings` with `{ model, input: [..] }` and
/// reads back `{ data: [{ embedding: [..] }, ..] }`. Works against Ollama,
/// llama.cpp, vLLM, OpenAI, or any compatible gateway.
pub struct HttpEmbedder {
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::blocking::Client,
}

impl HttpEmbedder {
    /// `base_url` is the endpoint root (no trailing `/v1/embeddings`). `api_key`
    /// is sent as `Authorization: Bearer` when `Some` (omit for local Ollama).
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        // Bound each embed call so a hung endpoint can't stall `mine`/`search`
        // forever. 60s covers a large batch; fall back to the default client if
        // the builder somehow fails.
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key,
            client,
        }
    }
}

#[derive(serde::Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
}

impl Embedder for HttpEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/v1/embeddings", self.base_url);
        let mut req = self.client.post(&url).json(&EmbedRequest {
            model: &self.model,
            input: texts,
        });
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().map_err(|e| EmbedError::Request(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(EmbedError::Status {
                status: status.as_u16(),
                body,
            });
        }
        let parsed: EmbedResponse = resp.json().map_err(|e| EmbedError::Decode(e.to_string()))?;
        if parsed.data.len() != texts.len() {
            return Err(EmbedError::CountMismatch {
                asked: texts.len(),
                got: parsed.data.len(),
            });
        }
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}

// ---------------------------------------------------------------------------
// StubEmbedder — deterministic, offline, test-only embeddings
// ---------------------------------------------------------------------------

/// Maps text to a fixed-dimension vector by hashing tokens into buckets.
/// Deterministic and offline — identical text yields identical vectors and
/// texts sharing words land near each other under cosine, which is enough to
/// exercise the store + k-NN ranking without a model server.
pub struct StubEmbedder {
    pub dim: usize,
}

impl StubEmbedder {
    /// Construct with a given vector dimension (`16` is plenty for tests).
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Embedder for StubEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|t| hash_embed(t, self.dim)).collect())
    }
}

/// A bag-of-words hash embedding: each lowercased word increments the bucket
/// it hashes into. Shared vocabulary → overlapping buckets → high cosine.
fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
        for b in word.to_ascii_lowercase().bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        v[(h as usize) % dim] += 1.0;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_is_deterministic() {
        let e = StubEmbedder::new(16);
        assert_eq!(
            e.embed_one("hello world").unwrap(),
            e.embed_one("hello world").unwrap()
        );
    }

    #[test]
    fn stub_shared_words_are_closer_than_disjoint() {
        let e = StubEmbedder::new(64);
        let a = e.embed_one("rust async runtime tokio").unwrap();
        let near = e.embed_one("rust async runtime executor").unwrap();
        let far = e.embed_one("banana smoothie recipe kitchen").unwrap();
        let cos = |x: &[f32], y: &[f32]| {
            let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
            let nx: f32 = x.iter().map(|p| p * p).sum::<f32>().sqrt();
            let ny: f32 = y.iter().map(|p| p * p).sum::<f32>().sqrt();
            dot / (nx * ny)
        };
        assert!(cos(&a, &near) > cos(&a, &far));
    }

    #[test]
    fn stub_empty_input_yields_empty_output() {
        let e = StubEmbedder::new(8);
        assert!(e.embed(&[]).unwrap().is_empty());
    }
}
