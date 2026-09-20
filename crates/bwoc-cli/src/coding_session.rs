//! Coding-session conversations for bare `bwoc` (runtime R4).
//!
//! Each directory gets its own folder under `~/.bwoc/sessions/`, named by a
//! hash of the canonical path, and each conversation in it is one
//! `<id>.json` file — the history `bwoc-harness --chat --session-file` reads
//! and rewrites after every turn. The folder also holds `dir.json` (which
//! directory it belongs to, so `bwoc session list --all` can say) and, for a
//! forked conversation, `<id>.meta.json` naming its parent.
//!
//! Ids are `YYYYMMDDTHHMMSSZ-xxxx`: creation time first so they sort
//! chronologically, a short suffix so two sessions started in the same second
//! differ. "Latest" is the conversation written most recently, by mtime.
//!
//! 3.2 kept one file per directory at `~/.bwoc/sessions/<hash>.json`. That
//! file moves into the folder, keeping `<hash>` as its id, the first time the
//! directory is opened, so an upgrade resumes the same conversation.
//!
//! Not the same thing as `bwoc sessions`, which lists running agent processes.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Which conversation a bare `bwoc` opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SessionPick {
    /// The most recently written conversation for this directory, or a new one.
    #[default]
    Latest,
    /// Always a new conversation.
    New,
    /// A conversation by id or unique id prefix.
    Id(String),
}

/// One stored conversation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionInfo {
    pub id: String,
    /// The directory the session belongs to; `None` for a 3.2 file whose
    /// directory has not been opened since the upgrade.
    pub cwd: Option<PathBuf>,
    /// First line of the first user message, shortened.
    pub title: String,
    /// User + assistant messages (tool traffic excluded).
    pub messages: usize,
    /// Last write, UTC ISO 8601.
    pub updated: String,
    /// The session this one was forked from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    mtime: Option<SystemTime>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DirMeta {
    cwd: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SessionError {
    #[error("no session matches `{0}` in this directory (see `bwoc session list`)")]
    NotFound(String),
    #[error("`{prefix}` matches {count} sessions in this directory; give more of the id")]
    Ambiguous { prefix: String, count: usize },
    #[error("this directory has no sessions yet")]
    Empty,
    #[error("{0}")]
    Io(String),
}

fn io_err(what: &str, path: &Path, e: std::io::Error) -> SessionError {
    SessionError::Io(format!("cannot {what} {}: {e}", path.display()))
}

/// The hash naming a directory's session folder (and its 3.2 file).
fn dir_key(cwd: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(cwd.to_string_lossy().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A fresh session id: creation time, then four hex digits that differ between
/// two ids minted in the same second by the same or another process.
pub fn new_id(now: SystemTime) -> String {
    let since = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let stamp: String = bwoc_core::time::format_iso8601(since.as_secs() as i64)
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    let salt = since.subsec_nanos() ^ std::process::id().rotate_left(16);
    format!("{stamp}-{:04x}", salt & 0xffff)
}

fn is_session_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".json")
        && !name.ends_with(".meta.json")
        && name != "dir.json"
        && !name.ends_with(".json.tmp")
}

/// Title and message count from a conversation file. A file that does not
/// parse still lists — as untitled — so it can be found and removed.
fn summarize(path: &Path) -> (String, usize) {
    let Some(messages) = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
    else {
        return ("(unreadable)".to_string(), 0);
    };
    let role = |m: &serde_json::Value| m.get("role").and_then(|r| r.as_str()).map(str::to_owned);
    let spoken = messages
        .iter()
        .filter(|m| matches!(role(m).as_deref(), Some("user" | "assistant")))
        .filter(|m| m.get("content").and_then(|c| c.as_str()).is_some())
        .count();
    let title = messages
        .iter()
        .find(|m| role(m).as_deref() == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .and_then(|c| c.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(|l| shorten(l, 60))
        .unwrap_or_else(|| "(empty)".to_string());
    (title, spoken)
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// The sessions of one directory.
pub struct SessionStore {
    root: PathBuf,
    dir: PathBuf,
    cwd: PathBuf,
}

impl SessionStore {
    /// Opens (without creating) the session folder for `cwd` under `bwoc_home`.
    pub fn new(bwoc_home: &Path, cwd: &Path) -> Self {
        let root = bwoc_home.join("sessions");
        Self {
            dir: root.join(dir_key(cwd)),
            root,
            cwd: cwd.to_path_buf(),
        }
    }

    /// Where session `id`'s conversation lives.
    pub fn path_of(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Creates the folder, records its directory, and moves a 3.2 conversation
    /// in. Idempotent.
    pub fn prepare(&self) -> Result<(), SessionError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| io_err("create", &self.dir, e))?;
        let marker = self.dir.join("dir.json");
        if !marker.exists() {
            let json = serde_json::to_string(&DirMeta {
                cwd: self.cwd.clone(),
            })
            .map_err(|e| SessionError::Io(e.to_string()))?;
            std::fs::write(&marker, json).map_err(|e| io_err("write", &marker, e))?;
        }
        // The 3.2 file keeps its id (the directory hash, which `new_id` never
        // mints), so an id copied from `session list` before the move still
        // resolves after it. A rename that finds nothing lost a race with
        // another `bwoc` opening the same directory, which did the move.
        let key = dir_key(&self.cwd);
        let legacy = self.root.join(format!("{key}.json"));
        let target = self.path_of(&key);
        if legacy.is_file() && !target.exists() {
            match std::fs::rename(&legacy, &target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_err("move", &legacy, e)),
            }
        }
        Ok(())
    }

    /// This directory's sessions, most recently written first.
    pub fn list(&self) -> Vec<SessionInfo> {
        let mut out = read_folder(&self.dir, Some(&self.cwd));
        let legacy = self.root.join(format!("{}.json", dir_key(&self.cwd)));
        if legacy.is_file() {
            out.extend(legacy_info(&legacy, Some(&self.cwd)));
        }
        sort_newest_first(&mut out);
        out
    }

    /// Resolves `pick` to a conversation path, minting an id for a new one.
    /// The file itself is written by the harness after the first turn.
    pub fn resolve(&self, pick: &SessionPick, now: SystemTime) -> Result<PathBuf, SessionError> {
        match pick {
            SessionPick::New => Ok(self.path_of(&new_id(now))),
            SessionPick::Latest => Ok(self.newest().unwrap_or_else(|| self.path_of(&new_id(now)))),
            SessionPick::Id(prefix) => self.find(prefix).map(|s| s.path),
        }
    }

    /// The most recently written conversation, by mtime alone: opening a
    /// session should not cost a parse of every conversation in the directory.
    fn newest(&self) -> Option<PathBuf> {
        let legacy = self.root.join(format!("{}.json", dir_key(&self.cwd)));
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_session_file(p))
            .chain(legacy.is_file().then_some(legacy))
            .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
            .max()
            .map(|(_, p)| p)
    }

    /// A session by id or unique id prefix.
    pub fn find(&self, prefix: &str) -> Result<SessionInfo, SessionError> {
        let prefix = prefix.trim();
        let all = self.list();
        if let Some(exact) = all.iter().find(|s| s.id == prefix) {
            return Ok(exact.clone());
        }
        let hits: Vec<_> = all
            .into_iter()
            .filter(|s| !prefix.is_empty() && s.id.starts_with(prefix))
            .collect();
        match hits.len() {
            0 => Err(SessionError::NotFound(prefix.to_string())),
            1 => Ok(hits.into_iter().next().expect("one hit")),
            count => Err(SessionError::Ambiguous {
                prefix: prefix.to_string(),
                count,
            }),
        }
    }

    /// Copies a session (the latest when `from` is `None`) into a new one that
    /// records its parent. Returns the new session.
    pub fn fork(&self, from: Option<&str>, now: SystemTime) -> Result<SessionInfo, SessionError> {
        self.prepare()?;
        let source = match from {
            Some(prefix) => self.find(prefix)?,
            None => self.list().into_iter().next().ok_or(SessionError::Empty)?,
        };
        let id = new_id(now);
        let target = self.path_of(&id);
        std::fs::copy(&source.path, &target).map_err(|e| io_err("copy", &source.path, e))?;
        let meta = self.dir.join(format!("{id}.meta.json"));
        let json = serde_json::to_string(&SessionMeta {
            parent: Some(source.id.clone()),
        })
        .map_err(|e| SessionError::Io(e.to_string()))?;
        std::fs::write(&meta, json).map_err(|e| io_err("write", &meta, e))?;
        self.find(&id)
    }

    /// Deletes a session and its metadata. Returns what was removed.
    pub fn remove(&self, prefix: &str) -> Result<SessionInfo, SessionError> {
        let session = self.find(prefix)?;
        std::fs::remove_file(&session.path).map_err(|e| io_err("remove", &session.path, e))?;
        let _ = std::fs::remove_file(self.dir.join(format!("{}.meta.json", session.id)));
        Ok(session)
    }
}

/// The TUI's in-session `/sessions`, `/session`, `/new` and `/fork`, backed by
/// this store (R5). `bwoc-tui` owns no on-disk layout, so it asks through this
/// trait object; the open conversation's id is remembered so the listing can
/// mark it.
pub struct TuiSessions {
    store: SessionStore,
    open: std::sync::Mutex<Option<String>>,
}

impl TuiSessions {
    pub fn new(store: SessionStore, open: Option<String>) -> Self {
        Self {
            store,
            open: std::sync::Mutex::new(open),
        }
    }

    fn remember(&self, id: &str) {
        if let Ok(mut open) = self.open.lock() {
            *open = Some(id.to_string());
        }
    }
}

impl bwoc_tui::SessionControl for TuiSessions {
    fn list(&self) -> Vec<bwoc_tui::SessionRow> {
        let open = self.open.lock().ok().and_then(|o| o.clone());
        self.store
            .list()
            .into_iter()
            .map(|s| bwoc_tui::SessionRow {
                current: Some(&s.id) == open.as_ref(),
                last: s
                    .updated
                    .split('T')
                    .nth(1)
                    .map(|t| t.trim_end_matches('Z').to_string())
                    .unwrap_or_else(|| s.updated.clone()),
                id: s.id,
                title: s.title,
                messages: s.messages,
            })
            .collect()
    }

    fn pick(&self, pick: &bwoc_tui::SessionPick) -> Result<(String, PathBuf), String> {
        self.store.prepare().map_err(|e| e.to_string())?;
        let now = SystemTime::now();
        let picked = match pick {
            bwoc_tui::SessionPick::New => {
                let path = self
                    .store
                    .resolve(&SessionPick::New, now)
                    .map_err(|e| e.to_string())?;
                let id = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (id, path)
            }
            bwoc_tui::SessionPick::Id(prefix) => {
                let found = self.store.find(prefix).map_err(|e| e.to_string())?;
                (found.id, found.path)
            }
            bwoc_tui::SessionPick::Fork(from) => {
                // `None` forks the conversation this TUI has open, not merely
                // the newest — the operator means "this one".
                let open = self.open.lock().ok().and_then(|o| o.clone());
                let from = from.clone().or(open);
                let forked = self
                    .store
                    .fork(from.as_deref(), now)
                    .map_err(|e| e.to_string())?;
                (forked.id, forked.path)
            }
        };
        self.remember(&picked.0);
        Ok(picked)
    }
}

/// Every stored session across all directories, most recently written first.
pub fn list_all(bwoc_home: &Path) -> Vec<SessionInfo> {
    let root = bwoc_home.join("sessions");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let cwd = std::fs::read_to_string(path.join("dir.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<DirMeta>(&s).ok())
                .map(|m| m.cwd);
            out.extend(read_folder(&path, cwd.as_deref()));
        } else if is_session_file(&path) {
            out.extend(legacy_info(&path, None));
        }
    }
    sort_newest_first(&mut out);
    out
}

fn read_folder(dir: &Path, cwd: Option<&Path>) -> Vec<SessionInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_session_file(p))
        .filter_map(|path| {
            let id = path.file_stem()?.to_str()?.to_string();
            let parent = std::fs::read_to_string(dir.join(format!("{id}.meta.json")))
                .ok()
                .and_then(|s| serde_json::from_str::<SessionMeta>(&s).ok())
                .and_then(|m| m.parent);
            Some(info(id, path, cwd, parent))
        })
        .collect()
}

/// A 3.2 per-directory file not yet moved into its folder. Its id is the
/// directory hash, which `bwoc` never mints, so it cannot collide.
fn legacy_info(path: &Path, cwd: Option<&Path>) -> Option<SessionInfo> {
    let id = path.file_stem()?.to_str()?.to_string();
    Some(info(id, path.to_path_buf(), cwd, None))
}

fn info(id: String, path: PathBuf, cwd: Option<&Path>, parent: Option<String>) -> SessionInfo {
    let (title, messages) = summarize(&path);
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let updated = mtime
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| bwoc_core::time::format_iso8601(d.as_secs() as i64))
        .unwrap_or_default();
    SessionInfo {
        id,
        cwd: cwd.map(Path::to_path_buf),
        title,
        messages,
        updated,
        parent,
        path,
        mtime,
    }
}

fn sort_newest_first(sessions: &mut [SessionInfo]) {
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| b.id.cmp(&a.id)));
}

// ---------------------------------------------------------------------------
// `bwoc session` subcommands
// ---------------------------------------------------------------------------

#[derive(Debug, clap::Subcommand)]
pub enum SessionCommand {
    /// Conversations for the current directory, newest first (`--all`: every
    /// directory). Bare `bwoc` resumes the first one listed.
    List {
        /// Every directory's sessions, not just this one's.
        #[arg(long)]
        all: bool,
        /// Emit `{ "sessions": [...] }` instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Copy a conversation into a new session (default: the latest) and print
    /// its id. The fork is now the latest, so bare `bwoc` resumes it; the
    /// original stays as it was and opens with `bwoc --session <parent-id>`.
    Fork {
        /// Session id or unique prefix.
        id: Option<String>,
    },
    /// Delete a conversation.
    Rm {
        /// Session id or unique prefix.
        id: String,
    },
}

pub fn run(cmd: SessionCommand) -> i32 {
    use crate::exit;
    let home = match crate::user_home::bwoc_home() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("bwoc: cannot locate ~/.bwoc: {e}");
            return exit::ERROR;
        }
    };
    let cwd = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc: cannot resolve the current directory: {e}");
            return exit::ERROR;
        }
    };
    let store = SessionStore::new(&home, &cwd);
    let fail = |e: SessionError| {
        eprintln!("bwoc: {e}");
        match e {
            SessionError::Io(_) => exit::ERROR,
            _ => exit::USAGE,
        }
    };
    match cmd {
        SessionCommand::List { all, json } => {
            let sessions = if all { list_all(&home) } else { store.list() };
            if json {
                println!("{}", serde_json::json!({ "sessions": sessions }));
            } else {
                print!("{}", render_table(&sessions, all));
            }
            exit::OK
        }
        SessionCommand::Fork { id } => match store.fork(id.as_deref(), SystemTime::now()) {
            Ok(s) => {
                // `fork` always records a parent; stay honest if it ever doesn't.
                match s.parent.as_deref() {
                    Some(parent) => println!(
                        "forked {parent} → {}\nbare `bwoc` now resumes the fork; \
                         the original opens with: bwoc --session {parent}",
                        s.id
                    ),
                    None => println!("forked → {}\nbare `bwoc` now resumes the fork", s.id),
                }
                exit::OK
            }
            Err(e) => fail(e),
        },
        SessionCommand::Rm { id } => match store.remove(&id) {
            Ok(s) => {
                println!("removed {} ({})", s.id, s.title);
                exit::OK
            }
            Err(e) => fail(e),
        },
    }
}

fn render_table(sessions: &[SessionInfo], with_dir: bool) -> String {
    use std::fmt::Write;
    if sessions.is_empty() {
        return if with_dir {
            "no sessions yet — run `bwoc` in a directory to start one\n".to_string()
        } else {
            "no sessions for this directory yet — run `bwoc` to start one\n".to_string()
        };
    }
    let mut out = String::new();
    for s in sessions {
        let updated = s
            .updated
            .replace('T', " ")
            .trim_end_matches('Z')
            .to_string();
        let _ = write!(out, "{:<22} {:<19} {:>4}  ", s.id, updated, s.messages);
        if with_dir {
            let dir = s
                .cwd
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(not opened since 3.2)".to_string());
            let _ = write!(out, "{dir}  ");
        }
        let _ = write!(out, "{}", s.title);
        if let Some(parent) = &s.parent {
            let _ = write!(out, "  (fork of {parent})");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn convo(first_user: &str) -> String {
        serde_json::json!([
            {"role": "user", "content": first_user},
            {"role": "assistant", "tool_calls": [{"id": "1"}]},
            {"role": "tool", "content": "ok", "tool_call_id": "1"},
            {"role": "assistant", "content": "done"},
        ])
        .to_string()
    }

    fn write_session(store: &SessionStore, id: &str, first_user: &str, mtime: SystemTime) {
        let path = store.path_of(id);
        std::fs::write(&path, convo(first_user)).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    #[test]
    fn ids_sort_by_creation_time_and_carry_no_separators_that_need_quoting() {
        let a = new_id(at(1_758_300_000));
        let b = new_id(at(1_758_300_061));
        assert!(a < b, "{a} < {b}");
        assert!(a.starts_with("20250919T"), "{a}");
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn latest_is_the_most_recently_written_session_and_new_mints_a_fresh_path() {
        let home = tempfile::tempdir().unwrap();
        let store = SessionStore::new(home.path(), Path::new("/src/a"));
        store.prepare().unwrap();
        // Older id, newer write: mtime decides, not the id.
        write_session(&store, "20250101T000000Z-0001", "older id", at(2_000));
        write_session(&store, "20250102T000000Z-0001", "newer id", at(1_000));
        let latest = store.resolve(&SessionPick::Latest, at(3_000)).unwrap();
        assert_eq!(latest, store.path_of("20250101T000000Z-0001"));
        let fresh = store.resolve(&SessionPick::New, at(3_000)).unwrap();
        assert!(!fresh.exists());
        assert_eq!(fresh.parent(), latest.parent());
    }

    #[test]
    fn an_empty_directory_resolves_latest_to_a_new_session() {
        let home = tempfile::tempdir().unwrap();
        let store = SessionStore::new(home.path(), Path::new("/src/a"));
        let path = store.resolve(&SessionPick::Latest, at(5)).unwrap();
        assert!(!path.exists());
        assert!(path.starts_with(home.path().join("sessions")));
    }

    #[test]
    fn a_3_2_conversation_moves_into_the_folder_and_stays_latest() {
        let home = tempfile::tempdir().unwrap();
        let cwd = Path::new("/src/legacy");
        let legacy = home
            .path()
            .join("sessions")
            .join(format!("{}.json", dir_key(cwd)));
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, convo("from 3.2")).unwrap();

        let store = SessionStore::new(home.path(), cwd);
        // Listed before the move, so `session list` shows it straight away —
        // and the id it shows still resolves once the file has moved.
        let before = store.list();
        assert_eq!(before[0].title, "from 3.2");
        assert_eq!(store.resolve(&SessionPick::Latest, at(1)).unwrap(), legacy);
        store.prepare().unwrap();
        assert!(!legacy.exists());
        let sessions = store.list();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "from 3.2");
        assert_eq!(sessions[0].id, before[0].id);
        assert_eq!(store.find(&before[0].id).unwrap().path, sessions[0].path);
        assert_eq!(
            store.resolve(&SessionPick::Latest, at(1)).unwrap(),
            sessions[0].path
        );
        // Idempotent.
        store.prepare().unwrap();
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn directories_do_not_see_each_others_sessions() {
        let home = tempfile::tempdir().unwrap();
        let a = SessionStore::new(home.path(), Path::new("/src/a"));
        let b = SessionStore::new(home.path(), Path::new("/src/b"));
        a.prepare().unwrap();
        b.prepare().unwrap();
        write_session(&a, "20250101T000000Z-000a", "in a", at(1));
        assert_eq!(a.list().len(), 1);
        assert!(b.list().is_empty());
        assert_eq!(
            b.find("20250101T000000Z-000a"),
            Err(SessionError::NotFound("20250101T000000Z-000a".into()))
        );
        let all = list_all(home.path());
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].cwd.as_deref(), Some(Path::new("/src/a")));
    }

    #[test]
    fn find_takes_a_unique_prefix_and_refuses_an_ambiguous_one() {
        let home = tempfile::tempdir().unwrap();
        let store = SessionStore::new(home.path(), Path::new("/src/a"));
        store.prepare().unwrap();
        write_session(&store, "20250101T000000Z-aaaa", "one", at(1));
        write_session(&store, "20250101T000000Z-aabb", "two", at(2));
        write_session(&store, "20250202T000000Z-cccc", "three", at(3));
        assert_eq!(store.find("202502").unwrap().title, "three");
        assert_eq!(store.find("20250101T000000Z-aab").unwrap().title, "two");
        assert_eq!(
            store.find("20250101"),
            Err(SessionError::Ambiguous {
                prefix: "20250101".into(),
                count: 2
            })
        );
        assert!(matches!(store.find(""), Err(SessionError::NotFound(_))));
    }

    #[test]
    fn summary_counts_spoken_messages_and_titles_from_the_first_user_line() {
        let home = tempfile::tempdir().unwrap();
        let store = SessionStore::new(home.path(), Path::new("/src/a"));
        store.prepare().unwrap();
        write_session(
            &store,
            "20250101T000000Z-0001",
            "\n  fix the parser\nplease",
            at(1),
        );
        let s = &store.list()[0];
        assert_eq!(s.title, "fix the parser");
        // user + final assistant text; the tool-call-only turn and the tool result don't count.
        assert_eq!(s.messages, 2);
        assert_eq!(shorten(&"x".repeat(80), 10).chars().count(), 10);
    }

    #[test]
    fn fork_copies_the_conversation_and_records_its_parent() {
        let home = tempfile::tempdir().unwrap();
        let store = SessionStore::new(home.path(), Path::new("/src/a"));
        assert_eq!(store.fork(None, at(9)).unwrap_err(), SessionError::Empty);
        write_session(&store, "20250101T000000Z-0001", "base", at(1));
        let forked = store.fork(None, at(1_758_300_000)).unwrap();
        assert_eq!(forked.parent.as_deref(), Some("20250101T000000Z-0001"));
        assert_eq!(forked.title, "base");
        // The copy is the newest write, so bare `bwoc` resumes the fork.
        assert_eq!(
            store.resolve(&SessionPick::Latest, at(1)).unwrap(),
            forked.path
        );
        assert_eq!(
            std::fs::read_to_string(&forked.path).unwrap(),
            convo("base")
        );
        // The fork is a session of its own: listed, removable, parent intact.
        assert_eq!(store.list().len(), 2);
        let removed = store.remove(&forked.id).unwrap();
        assert_eq!(removed.id, forked.id);
        assert!(!store.dir.join(format!("{}.meta.json", forked.id)).exists());
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn only_conversation_files_are_sessions() {
        for (name, is) in [
            ("20250101T000000Z-0001.json", true),
            ("20250101T000000Z-0001.meta.json", false),
            ("dir.json", false),
            ("20250101T000000Z-0001.json.tmp", false),
            ("notes.txt", false),
        ] {
            assert_eq!(is_session_file(Path::new(name)), is, "{name}");
        }
    }

    #[test]
    fn table_names_the_directory_only_across_directories() {
        let s = SessionInfo {
            id: "20250101T000000Z-0001".into(),
            cwd: None,
            title: "t".into(),
            messages: 3,
            updated: "2025-01-01T00:00:00Z".into(),
            parent: Some("p".into()),
            path: PathBuf::new(),
            mtime: None,
        };
        let here = render_table(std::slice::from_ref(&s), false);
        assert!(here.contains("2025-01-01 00:00:00"));
        assert!(!here.contains("3.2"));
        assert!(here.contains("(fork of p)"));
        assert!(render_table(&[s], true).contains("(not opened since 3.2)"));
        assert!(render_table(&[], false).contains("no sessions"));
    }
}
