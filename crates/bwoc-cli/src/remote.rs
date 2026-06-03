//! `bwoc remote link | list | status | unlink` — link agents to remote-control
//! sessions and manage those links.
//!
//! A **remote-control session** lets an agent's interactive session be driven
//! from elsewhere (e.g. Claude Code's Remote Control, reachable from
//! claude.ai / mobile). This command is **backend-neutral bookkeeping** over
//! that: it records, per agent, which external control session the agent is
//! linked to — it does not itself open or proxy the session.
//!
//! The link is stored at `<workspace>/.bwoc/remote/<agentId>.json`, mirroring
//! the `.bwoc/sessions/` marker convention. The `kind` field names the
//! remote-control mechanism — `claude-remote-control` is the first, and any
//! backend can declare its own kind later (Samānattatā — treat backends
//! equally; Claude is just the first implementation, not a special case).

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use bwoc_core::workspace::{AgentEntry, AgentsRegistry};

use crate::util::utc_now_iso8601;

/// Default remote-control mechanism when `--kind` is not given.
pub(crate) const DEFAULT_KIND: &str = "claude-remote-control";

/// One agent → remote-control session link. Serialized to
/// `.bwoc/remote/<agentId>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteLink {
    #[serde(rename = "agentId")]
    agent_id: String,
    /// Backend that exposes the session (e.g. `claude`).
    backend: String,
    /// Remote-control mechanism (e.g. `claude-remote-control`).
    kind: String,
    /// Opaque reference to the external session (id / token / handle).
    #[serde(rename = "sessionRef")]
    session_ref: String,
    /// Optional control URL for the session.
    #[serde(rename = "url", skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    /// When the link was recorded (ISO-8601 UTC).
    #[serde(rename = "linkedAt")]
    linked_at: String,
    /// Optional free-text note.
    #[serde(rename = "note", skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// Directory where remote-control link records live.
pub(crate) fn remote_dir(workspace: &Path) -> PathBuf {
    workspace.join(".bwoc/remote")
}

fn link_path(workspace: &Path, agent_id: &str) -> PathBuf {
    remote_dir(workspace).join(format!("{agent_id}.json"))
}

// ---------------------------------------------------------------------------
// CLI surface (consumed by main.rs)
// ---------------------------------------------------------------------------

pub struct RemoteArgs {
    pub action: RemoteAction,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE > ancestor walk.
    pub workspace: Option<PathBuf>,
    pub json: bool,
}

pub enum RemoteAction {
    /// `bwoc remote link <agent> <session-ref>` — record/replace a link.
    Link {
        agent: String,
        session_ref: String,
        backend: Option<String>,
        kind: Option<String>,
        url: Option<String>,
        note: Option<String>,
    },
    /// `bwoc remote list` — every recorded link.
    List,
    /// `bwoc remote status <agent>` — one agent's link.
    Status { agent: String },
    /// `bwoc remote unlink <agent>` — remove a link.
    Unlink { agent: String, yes: bool },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(args: RemoteArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc remote: no workspace found. Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc remote: failed to read agents.toml: {e}");
            return 1;
        }
    };

    match args.action {
        RemoteAction::Link {
            agent,
            session_ref,
            backend,
            kind,
            url,
            note,
        } => link(
            &workspace,
            &registry,
            &agent,
            session_ref,
            backend,
            kind,
            url,
            note,
            args.json,
        ),
        RemoteAction::List => list(&workspace, &registry, args.json),
        RemoteAction::Status { agent } => status(&workspace, &registry, &agent, args.json),
        RemoteAction::Unlink { agent, yes } => {
            unlink(&workspace, &registry, &agent, yes, args.json)
        }
    }
}

// ---------------------------------------------------------------------------
// link
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn link(
    workspace: &Path,
    registry: &AgentsRegistry,
    agent: &str,
    session_ref: String,
    backend: Option<String>,
    kind: Option<String>,
    url: Option<String>,
    note: Option<String>,
    json: bool,
) -> i32 {
    let Some(entry) = find_agent(registry, agent) else {
        eprintln!(
            "bwoc remote: no agent named '{agent}' in {}. Try `bwoc list`.",
            workspace.display()
        );
        return 2;
    };

    // Default the backend from the workspace registry (agents.toml), which
    // always records it — the manifest's `backend` field is optional and
    // usually absent, so reading it would mis-default non-Claude agents.
    let backend = backend.unwrap_or_else(|| entry.backend.clone());

    let record = RemoteLink {
        agent_id: entry.id.clone(),
        backend,
        kind: kind.unwrap_or_else(|| DEFAULT_KIND.to_string()),
        session_ref,
        url,
        linked_at: utc_now_iso8601(),
        note,
    };

    let dir = remote_dir(workspace);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("bwoc remote: cannot create {}: {e}", dir.display());
        return 1;
    }
    let path = link_path(workspace, &entry.id);
    let body = match serde_json::to_string_pretty(&record) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bwoc remote: serialize failed: {e}");
            return 1;
        }
    };
    if let Err(e) = std::fs::write(&path, format!("{body}\n")) {
        eprintln!("bwoc remote: cannot write {}: {e}", path.display());
        return 1;
    }

    if json {
        println!("{body}");
    } else {
        println!("✓ linked {} → {} session", record.agent_id, record.kind);
        println!("  backend:    {}", record.backend);
        println!("  sessionRef: {}", record.session_ref);
        if let Some(u) = &record.url {
            println!("  url:        {u}");
        }
    }
    0
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn list(workspace: &Path, registry: &AgentsRegistry, json: bool) -> i32 {
    let links = read_links(workspace);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&links).unwrap_or_default()
        );
        return 0;
    }

    if links.is_empty() {
        println!("(no remote-control links in {})", workspace.display());
        return 0;
    }

    // Warn about links whose agent is no longer registered (orphaned bookkeeping).
    let known: std::collections::HashSet<&str> =
        registry.agents.iter().map(|a| a.id.as_str()).collect();

    let agent_w = links
        .iter()
        .map(|l| l.agent_id.len())
        .max()
        .unwrap_or(5)
        .max(5);
    let kind_w = links.iter().map(|l| l.kind.len()).max().unwrap_or(4).max(4);
    println!(
        "{:<agent_w$}  {:<kind_w$}  {:<8}  LINKED AT",
        "AGENT", "KIND", "BACKEND"
    );
    for l in &links {
        let orphan = if known.contains(l.agent_id.as_str()) {
            ""
        } else {
            "  (orphaned — agent not registered)"
        };
        println!(
            "{:<agent_w$}  {:<kind_w$}  {:<8}  {}{}",
            l.agent_id, l.kind, l.backend, l.linked_at, orphan
        );
    }
    0
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

fn status(workspace: &Path, registry: &AgentsRegistry, agent: &str, json: bool) -> i32 {
    // Reject unknown agents up-front (exit 2). Orphaned-link inspection is
    // `bwoc remote list`'s job — `status` speaks only about registered agents.
    let Some(entry) = find_agent(registry, agent) else {
        eprintln!(
            "bwoc remote: no agent named '{agent}' in {}. Try `bwoc list`.",
            workspace.display()
        );
        return 2;
    };
    let id = entry.id.clone();
    let Some(link) = read_link(workspace, &id) else {
        // Agent exists but is unlinked → not an error (exit 0).
        if json {
            println!("{}", serde_json::json!({ "agent": id, "linked": false }));
        } else {
            eprintln!("bwoc remote: {id} has no remote-control link.");
        }
        return 0;
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&link).unwrap_or_default()
        );
        return 0;
    }

    println!("Agent:      {}", link.agent_id);
    println!("Kind:       {}", link.kind);
    println!("Backend:    {}", link.backend);
    println!("sessionRef: {}", link.session_ref);
    if let Some(u) = &link.url {
        println!("URL:        {u}");
    }
    println!("Linked at:  {}", link.linked_at);
    if let Some(n) = &link.note {
        println!("Note:       {n}");
    }
    0
}

// ---------------------------------------------------------------------------
// unlink
// ---------------------------------------------------------------------------

fn unlink(workspace: &Path, registry: &AgentsRegistry, agent: &str, yes: bool, json: bool) -> i32 {
    // Tolerate the agent being gone (orphaned link cleanup is the whole point).
    let id = match find_agent(registry, agent) {
        Some(e) => e.id.clone(),
        None => canonical_id(agent),
    };
    let path = link_path(workspace, &id);
    if !path.is_file() {
        if json {
            println!("{}", serde_json::json!({ "agent": id, "removed": false }));
        } else {
            eprintln!("bwoc remote: {id} has no remote-control link.");
        }
        return 0;
    }

    if !yes && !json && std::io::stdin().is_terminal() {
        eprint!("Remove remote-control link for {id}? [y/N] ");
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            return 1;
        }
        let answer = answer.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            eprintln!("Aborted.");
            return 1;
        }
    }

    if let Err(e) = std::fs::remove_file(&path) {
        eprintln!("bwoc remote: cannot remove {}: {e}", path.display());
        return 1;
    }
    if json {
        println!("{}", serde_json::json!({ "agent": id, "removed": true }));
    } else {
        println!("✓ unlinked {id}");
    }
    0
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Read every `.bwoc/remote/*.json` link, sorted by agent id.
fn read_links(workspace: &Path) -> Vec<RemoteLink> {
    let dir = remote_dir(workspace);
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(link) = serde_json::from_str::<RemoteLink>(&body) {
                out.push(link);
            }
        }
    }
    out.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    out
}

/// Read one agent's link, if present.
fn read_link(workspace: &Path, agent_id: &str) -> Option<RemoteLink> {
    let body = std::fs::read_to_string(link_path(workspace, agent_id)).ok()?;
    serde_json::from_str(&body).ok()
}

/// Normalize a user-supplied name to the canonical `agent-…` id.
fn canonical_id(name: &str) -> String {
    if name.starts_with("agent-") {
        name.to_string()
    } else {
        format!("agent-{name}")
    }
}

/// Find an agent by id, tolerating a missing `agent-` prefix.
fn find_agent<'a>(registry: &'a AgentsRegistry, name: &str) -> Option<&'a AgentEntry> {
    let lookup = canonical_id(name);
    registry.agents.iter().find(|a| a.id == lookup)
}

/// Resolution: explicit > BWOC_WORKSPACE env > ancestor walk for
/// `.bwoc/workspace.toml`. Mirrors the other command modules.
fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return Some(PathBuf::from(env_path));
        }
    }
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ws() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        // pid + a per-call counter → unique even when parallel tests start in
        // the same second (a timestamp alone collides).
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("bwoc-remote-test-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".bwoc")).unwrap();
        std::fs::write(d.join(".bwoc/workspace.toml"), "name = \"t\"\n").unwrap();
        d
    }

    #[test]
    fn canonical_id_adds_prefix() {
        assert_eq!(canonical_id("nezha"), "agent-nezha");
        assert_eq!(canonical_id("agent-nezha"), "agent-nezha");
    }

    #[test]
    fn link_roundtrip_via_files() {
        let ws = tmp_ws();
        std::fs::create_dir_all(remote_dir(&ws)).unwrap();
        let rec = RemoteLink {
            agent_id: "agent-x".into(),
            backend: "claude".into(),
            kind: DEFAULT_KIND.into(),
            session_ref: "sess-123".into(),
            url: Some("https://rc.example/x".into()),
            linked_at: "2026-06-03T00:00:00Z".into(),
            note: None,
        };
        let body = serde_json::to_string_pretty(&rec).unwrap();
        std::fs::write(link_path(&ws, "agent-x"), body).unwrap();

        let got = read_link(&ws, "agent-x").expect("link present");
        assert_eq!(got.session_ref, "sess-123");
        assert_eq!(got.kind, DEFAULT_KIND);
        assert_eq!(got.url.as_deref(), Some("https://rc.example/x"));

        let all = read_links(&ws);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].agent_id, "agent-x");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn read_link_absent_is_none() {
        let ws = tmp_ws();
        assert!(read_link(&ws, "agent-missing").is_none());
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn skip_serializing_omits_none_fields() {
        let rec = RemoteLink {
            agent_id: "agent-y".into(),
            backend: "claude".into(),
            kind: DEFAULT_KIND.into(),
            session_ref: "s".into(),
            url: None,
            linked_at: "t".into(),
            note: None,
        };
        let body = serde_json::to_string(&rec).unwrap();
        assert!(!body.contains("\"url\""));
        assert!(!body.contains("\"note\""));
    }
}
