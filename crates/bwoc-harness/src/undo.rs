//! Per-turn undo/redo of the files a session's tools changed (runtime R4b).
//!
//! The journal is an **edit journal**, not a snapshot of the worktree: each
//! entry is one file a file-mutating tool changed in a turn, with the text
//! before and after. That keeps the cost proportional to what the agent touched
//! and never puts a repository the operator owns under a second VCS — but it
//! also means a change made some other way (`run_command`, the operator's own
//! editor) is outside it, which [`Batch::conflicts`] reports rather than
//! silently overwriting.
//!
//! Layout, beside the conversation the harness was given:
//! `<session-file>.undo/<turn>.json`, plus `cursor` naming how many turns have
//! been undone. Text files only, each capped — a binary or very large write is
//! recorded as skipped so a later undo can say so instead of lying.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Largest file content journalled, per side. A bigger write is recorded as
/// skipped: keeping the whole file twice per turn is not worth the disk.
pub const MAX_JOURNAL_BYTES: usize = 1024 * 1024;

/// One file changed in a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Path as shown to the operator (relative to the workdir when possible).
    pub path: String,
    /// Absolute path actually written.
    pub full: PathBuf,
    /// Content before the change; `None` when the file did not exist.
    pub before: Option<String>,
    /// Content after the change; `None` when the tool removed it.
    pub after: Option<String>,
}

/// A turn's worth of entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Turn {
    pub entries: Vec<Entry>,
    /// Files changed in the turn that could not be journalled (binary, or over
    /// [`MAX_JOURNAL_BYTES`]) — named so an undo can say what it cannot restore.
    #[serde(default)]
    pub skipped: Vec<String>,
}

/// What an undo or redo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// Files restored, as shown to the operator.
    pub restored: Vec<String>,
    /// Files left alone because the content on disk is neither the state this
    /// step wrote nor the state it would restore — something else changed them.
    pub conflicts: Vec<String>,
    /// Files the turn changed but never journalled.
    pub skipped: Vec<String>,
}

/// The undo journal for one conversation.
pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    /// The journal beside `session_file`.
    pub fn for_session(session_file: &Path) -> Self {
        let mut dir = session_file.as_os_str().to_os_string();
        dir.push(".undo");
        Self {
            dir: PathBuf::from(dir),
        }
    }

    /// Record one turn. A turn with nothing to record writes nothing, so an
    /// undo skips past turns that changed no file. Recording a turn drops any
    /// redo stack — the timeline branched.
    pub fn record(&self, turn: &Turn) -> std::io::Result<()> {
        if turn.entries.is_empty() && turn.skipped.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)?;
        self.drop_undone()?;
        let next = self.turns().len();
        let path = self.dir.join(format!("{next:06}.json"));
        let json = serde_json::to_string(turn)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Restore the newest turn that has not been undone. `None` when there is
    /// nothing left to undo.
    pub fn undo(&self) -> std::io::Result<Option<Batch>> {
        let turns = self.turns();
        // The cursor is a file on disk: clamp it rather than trust it, so a
        // corrupted or hand-edited journal cannot panic the session.
        let cursor = self.cursor().min(turns.len());
        let Some(index) = turns.len().checked_sub(cursor + 1) else {
            return Ok(None);
        };
        let turn = self.read(&turns[index])?;
        let batch = apply(&turn, Direction::Undo);
        self.set_cursor(cursor + 1)?;
        Ok(Some(batch))
    }

    /// Reapply the most recently undone turn. `None` when nothing was undone.
    pub fn redo(&self) -> std::io::Result<Option<Batch>> {
        let turns = self.turns();
        let cursor = self.cursor().min(turns.len());
        if cursor == 0 {
            return Ok(None);
        }
        let index = turns.len() - cursor;
        let turn = self.read(&turns[index])?;
        let batch = apply(&turn, Direction::Redo);
        self.set_cursor(cursor - 1)?;
        Ok(Some(batch))
    }

    /// Turn files, oldest first.
    fn turns(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        out.sort();
        out
    }

    fn read(&self, path: &Path) -> std::io::Result<Turn> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// How many turns from the end have been undone.
    fn cursor(&self) -> usize {
        std::fs::read_to_string(self.dir.join("cursor"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    fn set_cursor(&self, value: usize) -> std::io::Result<()> {
        std::fs::write(self.dir.join("cursor"), value.to_string())
    }

    /// Forget the undone tail: a new turn after an undo replaces that future.
    fn drop_undone(&self) -> std::io::Result<()> {
        let turns = self.turns();
        let cursor = self.cursor().min(turns.len());
        if cursor == 0 {
            return Ok(());
        }
        for path in turns.iter().skip(turns.len() - cursor) {
            std::fs::remove_file(path)?;
        }
        self.set_cursor(0)
    }
}

enum Direction {
    Undo,
    Redo,
}

/// Restore each entry, refusing one whose file on disk is neither side of the
/// step (someone else changed it since).
fn apply(turn: &Turn, direction: Direction) -> Batch {
    let mut restored = Vec::new();
    let mut conflicts = Vec::new();
    for entry in &turn.entries {
        let (expect, write) = match direction {
            Direction::Undo => (&entry.after, &entry.before),
            Direction::Redo => (&entry.before, &entry.after),
        };
        let current = std::fs::read_to_string(&entry.full).ok();
        if current.as_deref() != expect.as_deref() {
            conflicts.push(entry.path.clone());
            continue;
        }
        let ok = match write {
            Some(content) => {
                if let Some(parent) = entry.full.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&entry.full, content).is_ok()
            }
            // The step created the file: undoing removes it again.
            None => std::fs::remove_file(&entry.full).is_ok(),
        };
        if ok {
            restored.push(entry.path.clone());
        } else {
            conflicts.push(entry.path.clone());
        }
    }
    Batch {
        restored,
        conflicts,
        skipped: turn.skipped.clone(),
    }
}

/// What reading a file for the journal produced.
#[derive(Debug, PartialEq, Eq)]
pub enum Journalled {
    /// Text the journal can restore; `None` when the file does not exist.
    Text(Option<String>),
    /// Binary, or over [`MAX_JOURNAL_BYTES`] — recorded as skipped instead.
    Unsupported,
}

/// Read a file for the journal. Only a genuinely absent file is `Text(None)`:
/// any other read failure (permissions, an I/O error, a directory) would make
/// the journal claim the file did not exist, and a later undo would delete
/// whatever is there.
pub fn journalled(path: &Path) -> Journalled {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Journalled::Text(None),
        Err(_) => Journalled::Unsupported,
        Ok(bytes) if bytes.len() > MAX_JOURNAL_BYTES => Journalled::Unsupported,
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Journalled::Text(Some(text)),
            Err(_) => Journalled::Unsupported,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(dir: &Path) -> Journal {
        Journal::for_session(&dir.join("session.json"))
    }

    fn entry(dir: &Path, name: &str, before: Option<&str>, after: Option<&str>) -> Entry {
        Entry {
            path: name.to_string(),
            full: dir.join(name),
            before: before.map(str::to_string),
            after: after.map(str::to_string),
        }
    }

    fn turn(entries: Vec<Entry>) -> Turn {
        Turn {
            entries,
            skipped: Vec::new(),
        }
    }

    #[test]
    fn undo_restores_the_previous_content_and_redo_puts_it_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "new").unwrap();
        let j = journal(dir);
        j.record(&turn(vec![entry(dir, "a.txt", Some("old"), Some("new"))]))
            .unwrap();

        let batch = j.undo().unwrap().expect("a turn to undo");
        assert_eq!(batch.restored, ["a.txt"]);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "old");

        let batch = j.redo().unwrap().expect("a turn to redo");
        assert_eq!(batch.restored, ["a.txt"]);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "new");
    }

    #[test]
    fn undoing_a_created_file_removes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("new.txt"), "hi").unwrap();
        let j = journal(dir);
        j.record(&turn(vec![entry(dir, "new.txt", None, Some("hi"))]))
            .unwrap();
        j.undo().unwrap().unwrap();
        assert!(!dir.join("new.txt").exists());
        j.redo().unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap(), "hi");
    }

    #[test]
    fn a_file_changed_behind_the_journal_is_a_conflict_not_an_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "new").unwrap();
        let j = journal(dir);
        j.record(&turn(vec![entry(dir, "a.txt", Some("old"), Some("new"))]))
            .unwrap();
        // Someone edited the file after the turn.
        std::fs::write(dir.join("a.txt"), "mine").unwrap();
        let batch = j.undo().unwrap().unwrap();
        assert_eq!(batch.conflicts, ["a.txt"]);
        assert!(batch.restored.is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "mine");
    }

    #[test]
    fn undo_walks_back_turn_by_turn_and_stops_at_the_start() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let j = journal(dir);
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        j.record(&turn(vec![entry(dir, "a.txt", None, Some("v1"))]))
            .unwrap();
        std::fs::write(dir.join("a.txt"), "v2").unwrap();
        j.record(&turn(vec![entry(dir, "a.txt", Some("v1"), Some("v2"))]))
            .unwrap();

        j.undo().unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "v1");
        j.undo().unwrap().unwrap();
        assert!(!dir.join("a.txt").exists());
        assert!(j.undo().unwrap().is_none(), "nothing left to undo");
    }

    #[test]
    fn recording_after_an_undo_drops_the_redo_future() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let j = journal(dir);
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        j.record(&turn(vec![entry(dir, "a.txt", None, Some("v1"))]))
            .unwrap();
        j.undo().unwrap().unwrap();

        std::fs::write(dir.join("b.txt"), "other").unwrap();
        j.record(&turn(vec![entry(dir, "b.txt", None, Some("other"))]))
            .unwrap();
        assert!(j.redo().unwrap().is_none(), "the old future is gone");
        // …and the new turn is still undoable.
        assert!(j.undo().unwrap().is_some());
    }

    #[test]
    fn a_turn_that_changed_nothing_is_not_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path());
        j.record(&turn(Vec::new())).unwrap();
        assert!(j.undo().unwrap().is_none());
    }

    #[test]
    fn a_corrupt_cursor_does_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let j = journal(dir);
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        j.record(&turn(vec![entry(dir, "a.txt", None, Some("v1"))]))
            .unwrap();
        // A cursor past the end of the journal (hand-edited, or turn files
        // removed behind us).
        std::fs::write(j.dir.join("cursor"), "99").unwrap();
        assert!(j.undo().unwrap().is_none());
        assert!(j.redo().unwrap().is_some());
    }

    #[test]
    fn an_unreadable_file_is_not_treated_as_missing() {
        let tmp = tempfile::tempdir().unwrap();
        // A directory is readable as a path but never as file content: the
        // journal must not report it as "did not exist".
        assert_eq!(journalled(tmp.path()), Journalled::Unsupported);
    }

    #[test]
    fn binary_and_oversized_files_are_not_journalled() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("b.bin");
        std::fs::write(&bin, [0xff, 0xfe]).unwrap();
        assert_eq!(journalled(&bin), Journalled::Unsupported);

        let big = tmp.path().join("big.txt");
        std::fs::write(&big, "x".repeat(MAX_JOURNAL_BYTES + 1)).unwrap();
        assert_eq!(journalled(&big), Journalled::Unsupported);

        let missing = tmp.path().join("nope.txt");
        assert_eq!(journalled(&missing), Journalled::Text(None));
    }
}
