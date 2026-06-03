//! Tier 2 deep-memory reference implementation for BWOC.
//!
//! A self-contained tool that speaks the backend-neutral contract defined by
//! `bwoc-core::deep_memory` — the three sub-commands `wake-up`, `search`, and
//! `mine` — over a local SQLite store with semantic (embedding) recall. Wire it
//! into any agent via `deepMemoryCmd` in `config.manifest.json`:
//!
//! ```text
//! deepMemoryCmd = "bwoc-deep-memory --db agents/agent-foo/.bwoc/deep.db \
//!                  --embed-url http://localhost:11434 --embed-model nomic-embed-text"
//! ```
//!
//! The crate is split into a seam-friendly library (this module tree) and a
//! thin `main.rs` that only does argument parsing + config resolution, so the
//! verb logic is unit-tested against a [`StubEmbedder`](embed::StubEmbedder)
//! and an in-memory [`Store`](store::Store) with no network or disk.

pub mod embed;
pub mod mine;
pub mod store;

use embed::{EmbedError, Embedder};
use store::{Memory, Store, StoreError};

/// Errors surfaced by the verb layer.
#[derive(Debug, thiserror::Error)]
pub enum VerbError {
    /// Embedding failed.
    #[error(transparent)]
    Embed(#[from] EmbedError),
    /// Store access failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Filesystem walk during `mine` failed.
    #[error("mine I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Outcome of a `mine` run — how many chunks were embedded and stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MineReport {
    /// Files that produced at least one chunk.
    pub files: usize,
    /// Chunks embedded and inserted.
    pub stored: usize,
}

/// `mine <path> --mode <mode>`: ingest session files into the store.
///
/// `ts` is the timestamp stamped on every inserted memory (Unix seconds) —
/// passed in by the caller so the verb stays deterministic under test.
pub fn mine(
    store: &Store,
    embedder: &dyn Embedder,
    path: &std::path::Path,
    mode: &str,
    ts: i64,
) -> Result<MineReport, VerbError> {
    let chunks = mine::collect(path)?;
    if chunks.is_empty() {
        return Ok(MineReport {
            files: 0,
            stored: 0,
        });
    }
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = embedder.embed(&texts)?;

    let mut sources = std::collections::HashSet::new();
    let mut stored = 0;
    for (chunk, vec) in chunks.iter().zip(vectors.iter()) {
        store.insert(&chunk.source, &chunk.text, mode, ts, vec)?;
        sources.insert(chunk.source.clone());
        stored += 1;
    }
    Ok(MineReport {
        files: sources.len(),
        stored,
    })
}

/// `search "<query>"`: embed the query, return the top-`limit` memories.
pub fn search(
    store: &Store,
    embedder: &dyn Embedder,
    query: &str,
    limit: usize,
) -> Result<Vec<Memory>, VerbError> {
    let q = embedder.embed_one(query)?;
    Ok(store.search(&q, limit)?)
}

/// `wake-up`: the `limit` most recent memories, for session-start injection.
pub fn wake_up(store: &Store, limit: usize) -> Result<Vec<Memory>, VerbError> {
    Ok(store.recent(limit)?)
}

/// Render memories as human-readable text (used by both `search` and
/// `wake-up`). Each entry is a `source`-tagged block.
pub fn render(memories: &[Memory], show_score: bool) -> String {
    if memories.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for m in memories {
        if show_score {
            out.push_str(&format!("# {} (score {:.3})\n", m.source, m.score));
        } else {
            out.push_str(&format!("# {}\n", m.source));
        }
        out.push_str(m.text.trim());
        out.push_str("\n\n");
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use embed::StubEmbedder;

    fn write_session(dir: &std::path::Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("session1.md"),
            "Decided to use rustls instead of OpenSSL for the HTTP client.\n\n\
             The sandbox profile embeds the canonical worktree path.",
        )
        .unwrap();
        std::fs::write(
            dir.join("session2.md"),
            "Banana smoothie needs frozen fruit and oat milk.",
        )
        .unwrap();
    }

    #[test]
    fn mine_then_search_finds_relevant() {
        let dir = std::env::temp_dir().join(format!("bwoc-verb-test-{}", std::process::id()));
        write_session(&dir);

        let store = Store::open_in_memory().unwrap();
        let emb = StubEmbedder::new(128);
        let report = mine(&store, &emb, &dir, "convos", 1000).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(report.files, 2);
        // session1's two short paragraphs coalesce into one chunk; session2 is
        // one chunk → 2 stored.
        assert_eq!(report.stored, 2);

        let hits = search(&store, &emb, "rustls HTTP client TLS", 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.to_lowercase().contains("rustls"));
    }

    #[test]
    fn wake_up_returns_recent_first() {
        let store = Store::open_in_memory().unwrap();
        let emb = StubEmbedder::new(16);
        store
            .insert("a", "old", "m", 10, &emb.embed_one("old").unwrap())
            .unwrap();
        store
            .insert("b", "new", "m", 20, &emb.embed_one("new").unwrap())
            .unwrap();
        let woken = wake_up(&store, 5).unwrap();
        assert_eq!(woken[0].text, "new");
    }

    #[test]
    fn mine_empty_path_is_zero_report() {
        let dir = std::env::temp_dir().join(format!("bwoc-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_in_memory().unwrap();
        let emb = StubEmbedder::new(8);
        let report = mine(&store, &emb, &dir, "m", 1).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            report,
            MineReport {
                files: 0,
                stored: 0
            }
        );
    }

    #[test]
    fn render_empty_is_empty() {
        assert_eq!(render(&[], false), "");
    }

    #[test]
    fn render_includes_source_and_score() {
        let m = Memory {
            id: 1,
            source: "s.md".into(),
            text: "body".into(),
            mode: "m".into(),
            ts: 1,
            score: 0.42,
        };
        let out = render(std::slice::from_ref(&m), true);
        assert!(out.contains("# s.md (score 0.420)"));
        assert!(out.contains("body"));
    }
}
