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
        // Fresh schema (new installs get everything at once). `embed_model`
        // stamps which model produced the vector so search can skip a
        // same-dimension-but-different-model row (#482).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                source      TEXT    NOT NULL,
                text        TEXT    NOT NULL,
                mode        TEXT    NOT NULL,
                ts          INTEGER NOT NULL,
                embedding   BLOB    NOT NULL,
                embed_model TEXT    NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_memories_ts ON memories(ts);",
        )?;

        // Migrate a pre-#482 table: add the column if it's missing. SQLite has no
        // `ADD COLUMN IF NOT EXISTS`, so probe `PRAGMA table_info` first.
        let has_embed_model = {
            let mut stmt = conn.prepare("PRAGMA table_info(memories)")?;
            let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
            let mut found = false;
            for c in cols {
                if c? == "embed_model" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_embed_model {
            conn.execute_batch(
                "ALTER TABLE memories ADD COLUMN embed_model TEXT NOT NULL DEFAULT '';",
            )?;
        }

        // One-time migration: collapse any duplicates a pre-#482 store already
        // accumulated (the same `chat-session.json` was re-mined every resume),
        // then enforce uniqueness so `INSERT OR IGNORE` makes re-mining
        // idempotent. Uniqueness is on `(source, text, embed_model)` — NOT just
        // `(source, text)` — so switching to a new embedding model re-embeds and
        // stores fresh rows under the new model instead of being ignored (and
        // then filtered out at search, which would make those memories vanish).
        // The full-table dedup scan runs only until the index exists, so a large
        // migrated store doesn't pay it on every open.
        let index_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1",
                ["idx_memories_source_text_model"],
                |_| Ok(()),
            )
            .is_ok();
        if !index_exists {
            conn.execute_batch(
                "DELETE FROM memories
                   WHERE id NOT IN (
                     SELECT MIN(id) FROM memories GROUP BY source, text, embed_model
                   );
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_source_text_model
                   ON memories(source, text, embed_model);",
            )?;
        }

        Ok(Self { conn })
    }

    /// Insert one memory, stamped with the embedding model that produced the
    /// vector. Returns `true` if a row was written, `false` if it was a
    /// duplicate `(source, text, embed_model)` and skipped — so callers can count
    /// dedups.
    ///
    /// `INSERT OR IGNORE` against the `(source, text, embed_model)` unique index
    /// makes re-mining the same session with the same model idempotent (#482) —
    /// a resumed session re-mines `chat-session.json`, and without this the store
    /// grew linearly per resume — while a **model switch** still re-embeds and
    /// stores fresh rows (the model is part of the key), so recall never silently
    /// disappears after changing `--embed-model`.
    pub fn insert(
        &self,
        source: &str,
        text: &str,
        mode: &str,
        ts: i64,
        embedding: &[f32],
        embed_model: &str,
    ) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO memories (source, text, mode, ts, embedding, embed_model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![source, text, mode, ts, vec_to_blob(embedding), embed_model],
        )?;
        Ok(changed > 0)
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

    /// Apply a retention policy and return how many rows were pruned.
    ///
    /// Two independent rules, applied as a **union** (a row is pruned if it
    /// matches *either*):
    /// - `older_than`: drop rows with `ts < older_than` (TTL by age).
    /// - `keep_newest`: keep only the newest `n` rows (by `ts` desc, `id` desc)
    ///   and drop the rest (cap by count).
    ///
    /// `dry_run` counts the victims without deleting — same number a real run
    /// would remove. With neither rule set the result is `0` (the CLI requires
    /// at least one rule, so this is a defensive no-op, never an accidental
    /// table wipe). Deletes run in a single transaction.
    pub fn prune(
        &self,
        older_than: Option<i64>,
        keep_newest: Option<usize>,
        dry_run: bool,
    ) -> Result<i64, StoreError> {
        use std::collections::BTreeSet;
        let mut victims: BTreeSet<i64> = BTreeSet::new();

        if let Some(cut) = older_than {
            let mut stmt = self.conn.prepare("SELECT id FROM memories WHERE ts < ?1")?;
            let ids = stmt.query_map([cut], |r| r.get::<_, i64>(0))?;
            for id in ids {
                victims.insert(id?);
            }
        }
        if let Some(keep) = keep_newest {
            // Rows beyond the newest `keep` (ts desc, id desc). `LIMIT -1` means
            // "no limit" in SQLite, so OFFSET alone selects everything after the
            // survivors. Clamp an absurd `keep` (> i64::MAX) to i64::MAX rather
            // than letting `as i64` wrap negative — a huge offset selects no
            // victims, the safe direction (prune nothing extra).
            let offset = i64::try_from(keep).unwrap_or(i64::MAX);
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM memories ORDER BY ts DESC, id DESC LIMIT -1 OFFSET ?1")?;
            let ids = stmt.query_map([offset], |r| r.get::<_, i64>(0))?;
            for id in ids {
                victims.insert(id?);
            }
        }

        if dry_run || victims.is_empty() {
            return Ok(victims.len() as i64);
        }

        // Atomic delete; return the number of rows actually removed (which can
        // be < victims.len() only if another process deleted some between
        // selection and here — truthful over the optimistic selection count).
        let txn = self.conn.unchecked_transaction()?;
        let mut deleted = 0i64;
        {
            let mut del = txn.prepare("DELETE FROM memories WHERE id = ?1")?;
            for id in &victims {
                deleted += del.execute([id])? as i64;
            }
        }
        txn.commit()?;
        Ok(deleted)
    }

    /// Top-`limit` memories by cosine similarity to `query_vec`, restricted to
    /// rows embedded by `embed_model` (or unstamped legacy rows).
    ///
    /// Brute force: loads every candidate row's embedding and scores in Rust.
    /// Two guards against silent mis-ranking (#482):
    /// - **model stamp** — a row whose `embed_model` is a *different* non-empty
    ///   model is excluded in SQL, since a same-dimension different model scores
    ///   plausibly but wrongly (the dimension check can't catch it). An empty
    ///   `embed_model` (legacy/unstamped) is kept, as is an empty `embed_model`
    ///   argument (an unstamped embedder matches only legacy rows).
    /// - **dimension** — a surviving row whose stored dimension differs from the
    ///   query scores `NaN` and is dropped below.
    pub fn search(
        &self,
        query_vec: &[f32],
        limit: usize,
        embed_model: &str,
    ) -> Result<Vec<Memory>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, text, mode, ts, embedding FROM memories
             WHERE embed_model = ?1 OR embed_model = ''",
        )?;
        let mut scored: Vec<Memory> = stmt
            .query_map([embed_model], |r| {
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
        s.insert("a.md", "first", "convos", 100, &[1.0, 0.0], "")
            .unwrap();
        s.insert("b.md", "second", "convos", 200, &[0.0, 1.0], "")
            .unwrap();
        assert_eq!(s.count().unwrap(), 2);
        let recent = s.recent(1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].text, "second"); // newest ts first
    }

    #[test]
    fn search_ranks_by_cosine() {
        let s = Store::open_in_memory().unwrap();
        s.insert("x", "near", "m", 1, &[1.0, 0.1], "").unwrap();
        s.insert("y", "far", "m", 2, &[-1.0, 0.0], "").unwrap();
        let hits = s.search(&[1.0, 0.0], 10, "").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text, "near");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn prune_older_than_drops_only_old_rows() {
        let s = Store::open_in_memory().unwrap();
        s.insert("a", "old", "m", 100, &[1.0, 0.0], "").unwrap();
        s.insert("b", "mid", "m", 200, &[1.0, 0.0], "").unwrap();
        s.insert("c", "new", "m", 300, &[1.0, 0.0], "").unwrap();
        // ts < 250 → drops "old" (100) and "mid" (200), keeps "new" (300).
        assert_eq!(s.prune(Some(250), None, false).unwrap(), 2);
        assert_eq!(s.count().unwrap(), 1);
        assert_eq!(s.recent(1).unwrap()[0].text, "new");
    }

    #[test]
    fn prune_keep_newest_caps_by_count() {
        let s = Store::open_in_memory().unwrap();
        for (src, ts) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
            s.insert(src, src, "m", ts, &[1.0, 0.0], "").unwrap();
        }
        // keep newest 2 → drops the 2 oldest.
        assert_eq!(s.prune(None, Some(2), false).unwrap(), 2);
        assert_eq!(s.count().unwrap(), 2);
        let kept = s.recent(10).unwrap();
        assert_eq!(kept[0].text, "d");
        assert_eq!(kept[1].text, "c");
    }

    #[test]
    fn prune_dry_run_counts_without_deleting() {
        let s = Store::open_in_memory().unwrap();
        s.insert("a", "old", "m", 100, &[1.0, 0.0], "").unwrap();
        s.insert("b", "new", "m", 300, &[1.0, 0.0], "").unwrap();
        assert_eq!(s.prune(Some(200), None, true).unwrap(), 1);
        assert_eq!(s.count().unwrap(), 2); // nothing actually removed
    }

    #[test]
    fn prune_union_of_both_rules_counts_each_row_once() {
        let s = Store::open_in_memory().unwrap();
        // ts: a=1 b=2 c=3 d=4. older_than=3 → {a,b}; keep_newest=1 → {a,b,c}.
        // Union is {a,b,c}; "a"/"b" overlap and must not be double-counted.
        for (src, ts) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
            s.insert(src, src, "m", ts, &[1.0, 0.0], "").unwrap();
        }
        assert_eq!(s.prune(Some(3), Some(1), false).unwrap(), 3);
        assert_eq!(s.count().unwrap(), 1);
        assert_eq!(s.recent(1).unwrap()[0].text, "d");
    }

    #[test]
    fn prune_no_rule_is_noop() {
        let s = Store::open_in_memory().unwrap();
        s.insert("a", "keep", "m", 1, &[1.0, 0.0], "").unwrap();
        assert_eq!(s.prune(None, None, false).unwrap(), 0);
        assert_eq!(s.count().unwrap(), 1);
    }

    #[test]
    fn search_skips_dimension_mismatch() {
        let s = Store::open_in_memory().unwrap();
        s.insert("ok", "good", "m", 1, &[1.0, 0.0], "").unwrap();
        s.insert("bad", "wrong-dim", "m", 2, &[1.0, 0.0, 0.0], "")
            .unwrap();
        let hits = s.search(&[1.0, 0.0], 10, "").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "good");
    }

    #[test]
    fn insert_dedups_same_source_text() {
        // Re-mining the same session must not grow the store (#482).
        let s = Store::open_in_memory().unwrap();
        assert!(
            s.insert("sess.json", "hi", "m", 1, &[1.0, 0.0], "")
                .unwrap()
        );
        // Same (source, text) again — even with a newer ts — is a skip.
        assert!(
            !s.insert("sess.json", "hi", "m", 2, &[1.0, 0.0], "")
                .unwrap()
        );
        assert_eq!(s.count().unwrap(), 1);
        // A different text at the same source is a distinct row.
        assert!(
            s.insert("sess.json", "bye", "m", 3, &[0.0, 1.0], "")
                .unwrap()
        );
        assert_eq!(s.count().unwrap(), 2);
    }

    #[test]
    fn model_switch_re_embeds_instead_of_vanishing() {
        // #491: after switching embed_model, re-mining the same (source, text)
        // must store a fresh row under the new model — not be ignored and then
        // filtered out of search, which would make the memory disappear.
        let s = Store::open_in_memory().unwrap();
        assert!(
            s.insert("sess", "fact", "m", 1, &[1.0, 0.0], "old")
                .unwrap()
        );
        // Same (source, text) but a NEW model → a distinct row, not a skip.
        assert!(
            s.insert("sess", "fact", "m", 2, &[0.0, 1.0], "new")
                .unwrap()
        );
        assert_eq!(s.count().unwrap(), 2);
        // A search on the new model still finds the memory.
        let hits = s.search(&[0.0, 1.0], 10, "new").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "fact");
        // Re-mining again with the new model is idempotent.
        assert!(
            !s.insert("sess", "fact", "m", 3, &[0.0, 1.0], "new")
                .unwrap()
        );
        assert_eq!(s.count().unwrap(), 2);
    }

    #[test]
    fn search_filters_out_a_different_embed_model() {
        // Two same-dimension rows from different models: a search stamped with
        // model "A" must not rank the "B" row (silent mis-rank guard, #482).
        let s = Store::open_in_memory().unwrap();
        s.insert("a", "from-a", "m", 1, &[1.0, 0.0], "model-a")
            .unwrap();
        s.insert("b", "from-b", "m", 2, &[1.0, 0.0], "model-b")
            .unwrap();
        s.insert("c", "legacy", "m", 3, &[1.0, 0.0], "").unwrap(); // unstamped

        let hits = s.search(&[1.0, 0.0], 10, "model-a").unwrap();
        let texts: Vec<&str> = hits.iter().map(|m| m.text.as_str()).collect();
        assert!(texts.contains(&"from-a"), "same-model row kept: {texts:?}");
        assert!(
            texts.contains(&"legacy"),
            "unstamped legacy row kept: {texts:?}"
        );
        assert!(
            !texts.contains(&"from-b"),
            "different-model row excluded: {texts:?}"
        );
    }
}
