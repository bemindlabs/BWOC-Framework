//! Local memory store — a single SQLite file holding text + its embedding.
//!
//! v1 ranks with brute-force cosine similarity in Rust (vectors stored as a
//! little-endian `f32` BLOB). For a single agent's memories this is trivially
//! fast and carries no native-extension build risk; the seam can be swapped for
//! `sqlite-vec` k-NN later without changing the public surface — Anattā (no
//! clinging to the v1 storage detail).

use std::path::Path;

use rusqlite::Connection;

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Any underlying SQLite failure.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Filesystem failure preparing the store path (e.g. parent dir creation).
    #[error("store I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// One stored memory plus, in query results, its similarity to the query.
#[derive(Debug, Clone, PartialEq)]
pub struct Memory {
    /// Row id (autoincrement).
    pub id: i64,
    /// Origin of the text (e.g. a session file path).
    pub source: String,
    /// The remembered text chunk.
    pub text: String,
    /// Tool-defined mode tag passed to `mine` (e.g. `"convos"`).
    pub mode: String,
    /// Unix seconds when the memory was written.
    pub ts: i64,
    /// Cosine similarity to the query — `0.0` for non-search reads.
    pub score: f32,
}

/// A handle to the SQLite-backed store.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) the store at `path`, ensuring the schema.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            // Skip the empty parent of a bare filename (`create_dir_all("")`
            // errors); surface real creation failures instead of masking them.
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open an in-memory store (tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                source    TEXT    NOT NULL,
                text      TEXT    NOT NULL,
                mode      TEXT    NOT NULL,
                ts        INTEGER NOT NULL,
                embedding BLOB    NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memories_ts ON memories(ts);",
        )?;
        Ok(Self { conn })
    }

    /// Insert one memory. Returns the new row id.
    pub fn insert(
        &self,
        source: &str,
        text: &str,
        mode: &str,
        ts: i64,
        embedding: &[f32],
    ) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO memories (source, text, mode, ts, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![source, text, mode, ts, vec_to_blob(embedding)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Total rows in the store.
    pub fn count(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?)
    }

    /// The `limit` most recently written memories (ts desc). Used by `wake-up`.
    pub fn recent(&self, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, text, mode, ts FROM memories
             ORDER BY ts DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(Memory {
                id: r.get(0)?,
                source: r.get(1)?,
                text: r.get(2)?,
                mode: r.get(3)?,
                ts: r.get(4)?,
                score: 0.0,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(StoreError::from)
    }

    /// Top-`limit` memories by cosine similarity to `query_vec`.
    ///
    /// Brute force: loads every row's embedding and scores in Rust. Rows whose
    /// stored dimension differs from the query (e.g. the embedding model
    /// changed) are skipped rather than silently mis-scored.
    pub fn search(&self, query_vec: &[f32], limit: usize) -> Result<Vec<Memory>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source, text, mode, ts, embedding FROM memories")?;
        let mut scored: Vec<Memory> = stmt
            .query_map([], |r| {
                let blob: Vec<u8> = r.get(5)?;
                let emb = blob_to_vec(&blob);
                let score = cosine(query_vec, &emb);
                Ok(Memory {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    text: r.get(2)?,
                    mode: r.get(3)?,
                    ts: r.get(4)?,
                    score,
                })
            })?
            .collect::<Result<_, _>>()?;

        // NaN (dimension mismatch / zero vector) sinks to the bottom.
        scored.retain(|m| !m.score.is_nan());
        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        scored.truncate(limit);
        Ok(scored)
    }
}

/// Pack an `f32` slice into little-endian bytes.
fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Unpack little-endian bytes back into an `f32` vector. A length that is not a
/// multiple of 4 means a corrupt/partial BLOB — return an empty vector so the
/// row scores `NaN` (dimension mismatch) and is skipped, rather than silently
/// decoding into a wrong-length vector that could mis-score.
fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    if b.len() % 4 != 0 {
        return Vec::new();
    }
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity. Returns `NaN` when dimensions differ or either vector is
/// zero-length / zero-magnitude — callers filter those out.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::NAN;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return f32::NAN;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrips() {
        let v = vec![1.0f32, -2.5, 3.25, 0.0];
        assert_eq!(blob_to_vec(&vec_to_blob(&v)), v);
    }

    #[test]
    fn blob_non_multiple_of_four_is_empty() {
        // 5 bytes → would otherwise decode to one f32 + dropped trailing byte,
        // a wrong-length vector that could coincidentally match. Reject it.
        assert!(blob_to_vec(&[0u8, 0, 0, 0, 7]).is_empty());
    }

    #[test]
    fn cosine_identical_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_dimension_mismatch_is_nan() {
        assert!(cosine(&[1.0, 2.0], &[1.0]).is_nan());
    }

    #[test]
    fn insert_count_recent() {
        let s = Store::open_in_memory().unwrap();
        s.insert("a.md", "first", "convos", 100, &[1.0, 0.0])
            .unwrap();
        s.insert("b.md", "second", "convos", 200, &[0.0, 1.0])
            .unwrap();
        assert_eq!(s.count().unwrap(), 2);
        let recent = s.recent(1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].text, "second"); // newest ts first
    }

    #[test]
    fn search_ranks_by_cosine() {
        let s = Store::open_in_memory().unwrap();
        s.insert("x", "near", "m", 1, &[1.0, 0.1]).unwrap();
        s.insert("y", "far", "m", 2, &[-1.0, 0.0]).unwrap();
        let hits = s.search(&[1.0, 0.0], 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text, "near");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn search_skips_dimension_mismatch() {
        let s = Store::open_in_memory().unwrap();
        s.insert("ok", "good", "m", 1, &[1.0, 0.0]).unwrap();
        s.insert("bad", "wrong-dim", "m", 2, &[1.0, 0.0, 0.0])
            .unwrap();
        let hits = s.search(&[1.0, 0.0], 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "good");
    }
}
