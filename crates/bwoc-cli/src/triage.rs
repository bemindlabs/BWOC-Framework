//! `bwoc triage <agent>` — rule-based coordinator loop over an agent's gateway
//! inbox (issue #296).
//!
//! The gateway receiver appends remote messages to `.bwoc/inbox.jsonl`, but a
//! relay-only orchestrator has nothing that *consumes* them, so they accumulate
//! forever (busaba had 97 unprocessed). This is the missing processor: it reads
//! new envelopes from a persisted byte cursor (`.bwoc/inbox.triage.cursor`),
//! classifies each by **deterministic rules** — no model is run on the untrusted
//! message, so it is safe even for an ambient (`cli`) backend the harness cannot
//! confine — emits a receipt to `.bwoc/inbox.triage.jsonl`, advances the cursor,
//! and prints a digest.
//!
//! Delivery is **at-least-once per pass**: the cursor is persisted only after a
//! pass's forwards + receipts, so a clean run processes each message exactly
//! once, but a crash mid-pass re-processes that pass's messages on restart (a
//! duplicate forward / receipt is possible). Actions are deliberately
//! idempotent-friendly (forwards are content-addressable appends; escalate is a
//! digest line) so a replay is harmless rather than corrupting.
//!
//! Actions: `ack` (record + drop), `escalate` (flag for the operator in the
//! digest), `forward` (re-deliver the envelope to another agent's inbox via the
//! shared `AgentEntry::inbox_path` resolver). Rules come from an optional
//! `interconnect/triage.toml`; the default action is `escalate`.

use std::path::{Path, PathBuf};

use bwoc_core::workspace::AgentsRegistry;

pub struct TriageArgs {
    /// Agent whose inbox to triage. Matches by id ("agent-foo") or bare name.
    pub agent: String,
    pub workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON digest instead of the human one.
    pub json: bool,
    /// Poll continuously (process the backlog, then watch for new messages).
    /// Without it, drain the current backlog once and exit.
    pub loop_: bool,
    /// Classify + digest only: do not write receipts, forward, or advance the
    /// cursor. A safe preview of what a real run would do.
    pub dry_run: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum TriageError {
    #[error(
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
    )]
    NoWorkspace,
    #[error("no agent named '{name}' in workspace {workspace}")]
    NotFound { name: String, workspace: PathBuf },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
    #[error("invalid interconnect/triage.toml: {0}")]
    Config(String),
}

pub fn run(args: TriageArgs) -> i32 {
    match triage(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc triage: {e}");
            match e {
                TriageError::NoWorkspace | TriageError::NotFound { .. } => 2,
                _ => 1,
            }
        }
    }
}

// ── Triage rules ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    /// Record + drop (no operator attention needed).
    Ack,
    /// Flag for the operator in the digest.
    Escalate,
    /// Re-deliver the envelope to `target`'s inbox.
    Forward(String),
}

impl Action {
    fn label(&self) -> &str {
        match self {
            Action::Ack => "ack",
            Action::Escalate => "escalate",
            Action::Forward(_) => "forward",
        }
    }
}

/// One classification rule from `interconnect/triage.toml`.
struct Rule {
    /// Substring matched against the message text, or `from:<id>` matched
    /// against the sender id.
    pattern: String,
    action: Action,
}

struct Config {
    default_action: Action,
    rules: Vec<Rule>,
}

impl Config {
    /// The built-in config: no rules, every message escalates. Used when no
    /// `interconnect/triage.toml` exists — the safe default (nothing is silently
    /// dropped; the operator sees every coordination request).
    fn builtin() -> Self {
        Self {
            default_action: Action::Escalate,
            rules: Vec::new(),
        }
    }

    /// Load `<agent_dir>/interconnect/triage.toml`, or the built-in default when
    /// the file is absent.
    fn load(agent_dir: &Path) -> Result<Self, TriageError> {
        let path = agent_dir.join("interconnect/triage.toml");
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            // Absent → the built-in default. A *present-but-unreadable* file
            // (permission, transient I/O) is a real error, not "no config" —
            // surface it rather than silently escalating everything.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::builtin()),
            Err(e) => {
                return Err(TriageError::Config(format!(
                    "cannot read {}: {e}",
                    path.display()
                )));
            }
        };
        let toml_cfg: TriageConfigToml =
            toml::from_str(&raw).map_err(|e| TriageError::Config(e.to_string()))?;
        let default_action = parse_action(&toml_cfg.default_action, None).ok_or_else(|| {
            TriageError::Config(format!("bad default_action: {}", toml_cfg.default_action))
        })?;
        let mut rules = Vec::with_capacity(toml_cfg.rules.len());
        for r in toml_cfg.rules {
            let action = parse_action(&r.action, r.target.as_deref()).ok_or_else(|| {
                TriageError::Config(format!(
                    "bad rule action `{}`{}",
                    r.action,
                    if r.action == "forward" {
                        " (forward needs a `target`)"
                    } else {
                        ""
                    }
                ))
            })?;
            rules.push(Rule {
                pattern: r.pattern,
                action,
            });
        }
        Ok(Self {
            default_action,
            rules,
        })
    }

    /// Classify one envelope: the first matching rule wins, else the default.
    fn classify(&self, from: &str, message: &str) -> Action {
        for rule in &self.rules {
            let hit = if let Some(id) = rule.pattern.strip_prefix("from:") {
                from == id
            } else {
                message.contains(&rule.pattern)
            };
            if hit {
                return rule.action.clone();
            }
        }
        self.default_action.clone()
    }
}

fn parse_action(s: &str, target: Option<&str>) -> Option<Action> {
    match s {
        "ack" => Some(Action::Ack),
        "escalate" => Some(Action::Escalate),
        "forward" => target.map(|t| Action::Forward(t.to_string())),
        _ => None,
    }
}

#[derive(serde::Deserialize)]
struct TriageConfigToml {
    #[serde(default = "default_action_str")]
    default_action: String,
    #[serde(default, rename = "rule")]
    rules: Vec<RuleToml>,
}

fn default_action_str() -> String {
    "escalate".to_string()
}

#[derive(serde::Deserialize)]
struct RuleToml {
    pattern: String,
    action: String,
    #[serde(default)]
    target: Option<String>,
}

// ── Driver ────────────────────────────────────────────────────────────────────

/// The outcome of triaging one envelope — also the shape of a receipt line.
struct Triaged {
    ts: String,
    from: String,
    message: String,
    action: Action,
}

fn triage(args: TriageArgs) -> Result<(), TriageError> {
    let workspace = resolve_workspace(args.workspace.clone()).ok_or(TriageError::NoWorkspace)?;
    let registry = AgentsRegistry::load(&workspace)?;
    let lookup_id = if args.agent.starts_with("agent-") {
        args.agent.clone()
    } else {
        format!("agent-{}", args.agent)
    };
    let entry = registry
        .agents
        .iter()
        .find(|a| a.id == lookup_id)
        .ok_or_else(|| TriageError::NotFound {
            name: args.agent.clone(),
            workspace: workspace.clone(),
        })?;

    let agent_dir = entry.dir(&workspace);
    let inbox_path = entry.inbox_path(&workspace);
    let cursor_path = agent_dir.join(".bwoc/inbox.triage.cursor");
    let receipts_path = agent_dir.join(".bwoc/inbox.triage.jsonl");
    let config = Config::load(&agent_dir)?;

    // First pass over the accumulated backlog (cursor starts at 0 — the whole
    // point is to drain what piled up, unlike the daemon which starts at EOF).
    let mut cursor = load_cursor(&cursor_path).unwrap_or(0);
    cursor = drain(
        &entry.id,
        &inbox_path,
        &cursor_path,
        &receipts_path,
        &workspace,
        &registry,
        &config,
        cursor,
        args.json,
        args.dry_run,
    )?;

    if !args.loop_ {
        return Ok(());
    }
    // Loop mode: poll for new envelopes appended past the cursor.
    use std::time::Duration;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let size = std::fs::metadata(&inbox_path).map(|m| m.len()).unwrap_or(0);
        if size <= cursor {
            if size < cursor {
                cursor = size; // truncated — reset
            }
            continue;
        }
        cursor = drain(
            &entry.id,
            &inbox_path,
            &cursor_path,
            &receipts_path,
            &workspace,
            &registry,
            &config,
            cursor,
            args.json,
            args.dry_run,
        )?;
    }
}

/// Process every complete envelope from `cursor` to EOF, write receipts, perform
/// each action, advance the cursor, and print a digest. Returns the new cursor.
#[allow(clippy::too_many_arguments)]
fn drain(
    agent_id: &str,
    inbox_path: &Path,
    cursor_path: &Path,
    receipts_path: &Path,
    workspace: &Path,
    registry: &AgentsRegistry,
    config: &Config,
    cursor: u64,
    json: bool,
    dry_run: bool,
) -> Result<u64, TriageError> {
    let (consumed, envelopes) = read_envelopes_from(inbox_path, cursor)?;
    if envelopes.is_empty() {
        return Ok(cursor);
    }

    let mut triaged: Vec<Triaged> = Vec::with_capacity(envelopes.len());
    for env in &envelopes {
        let from = env.get("from").and_then(|v| v.as_str()).unwrap_or("—");
        let ts = env.get("ts").and_then(|v| v.as_str()).unwrap_or("—");
        let message = env.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let mut action = config.classify(from, message);
        if let Action::Forward(target) = &action {
            match resolve_target(registry, target) {
                // Target missing: a recorded-forward that never delivers would
                // drop the message (cursor advances, nothing receives it).
                // Downgrade to escalate so it's surfaced, not lost.
                None => {
                    eprintln!(
                        "bwoc triage: warning — forward target '{target}' not in workspace; escalating instead"
                    );
                    action = Action::Escalate;
                }
                Some(entry) if !dry_run => forward(env, entry, agent_id, workspace)?,
                Some(_) => {} // dry-run: would forward, but don't.
            }
        }
        triaged.push(Triaged {
            ts: ts.to_string(),
            from: from.to_string(),
            message: message.to_string(),
            action,
        });
    }

    if !dry_run {
        append_receipts(receipts_path, agent_id, &triaged)?;
        save_cursor(cursor_path, cursor + consumed)?;
    }

    emit_digest(agent_id, &triaged, json, dry_run);
    Ok(cursor + consumed)
}

/// Resolve a forward `target` (id or bare name) to a registered agent.
fn resolve_target<'a>(
    registry: &'a AgentsRegistry,
    target: &str,
) -> Option<&'a bwoc_core::workspace::AgentEntry> {
    let id = if target.starts_with("agent-") {
        target.to_string()
    } else {
        format!("agent-{target}")
    };
    registry.agents.iter().find(|a| a.id == id)
}

/// Re-deliver an envelope to `target_entry`'s inbox via the shared resolver,
/// tagging it with the triaging agent + a `forwarded_by` marker so loops are
/// visible. The caller has already confirmed the target exists.
fn forward(
    env: &serde_json::Value,
    target_entry: &bwoc_core::workspace::AgentEntry,
    triaged_by: &str,
    workspace: &Path,
) -> Result<(), TriageError> {
    let mut forwarded = env.clone();
    if let Some(obj) = forwarded.as_object_mut() {
        obj.insert(
            "forwarded_by".to_string(),
            serde_json::Value::String(triaged_by.to_string()),
        );
    }
    let path = target_entry.inbox_path(workspace);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(
        f,
        "{}",
        serde_json::to_string(&forwarded).unwrap_or_default()
    )?;
    Ok(())
}

fn append_receipts(
    receipts_path: &Path,
    agent_id: &str,
    triaged: &[Triaged],
) -> Result<(), TriageError> {
    if let Some(parent) = receipts_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(receipts_path)?;
    for t in triaged {
        let mut rec = serde_json::json!({
            "agent": agent_id,
            "ts": t.ts,
            "from": t.from,
            "action": t.action.label(),
            "message": truncate(&t.message, 200),
        });
        if let Action::Forward(target) = &t.action {
            rec["target"] = serde_json::Value::String(target.clone());
        }
        writeln!(f, "{}", serde_json::to_string(&rec).unwrap_or_default())?;
    }
    Ok(())
}

fn emit_digest(agent_id: &str, triaged: &[Triaged], json: bool, dry_run: bool) {
    let (mut acked, mut escalated, mut forwarded) = (0usize, 0usize, 0usize);
    for t in triaged {
        match t.action {
            Action::Ack => acked += 1,
            Action::Escalate => escalated += 1,
            Action::Forward(_) => forwarded += 1,
        }
    }
    if json {
        let escalations: Vec<serde_json::Value> = triaged
            .iter()
            .filter(|t| t.action == Action::Escalate)
            .map(|t| {
                serde_json::json!({
                    "ts": t.ts, "from": t.from, "message": truncate(&t.message, 200),
                })
            })
            .collect();
        let value = serde_json::json!({
            "agent": agent_id,
            "dry_run": dry_run,
            "processed": triaged.len(),
            "acked": acked,
            "escalated": escalated,
            "forwarded": forwarded,
            "escalations": escalations,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
        );
        return;
    }

    let tag = if dry_run { " (dry-run)" } else { "" };
    println!();
    println!(
        "Triaged {} message(s) for {agent_id}{tag}: {acked} ack, {escalated} escalate, {forwarded} forward",
        triaged.len()
    );
    if escalated > 0 {
        println!();
        println!("Needs attention:");
        for t in triaged.iter().filter(|t| t.action == Action::Escalate) {
            println!("  [{}] {} — {}", t.ts, t.from, truncate(&t.message, 100));
        }
    }
    println!();
}

fn truncate(s: &str, max: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() <= max {
        return one_line;
    }
    let mut out: String = one_line.chars().take(max).collect();
    out.push('…');
    out
}

// ── Cursor + reader (mirrors the daemon's byte-offset scheme) ──────────────────

fn load_cursor(path: &Path) -> Option<u64> {
    // A missing cursor is the normal first run (start at 0, drain the backlog).
    // A present-but-unreadable/malformed cursor is different: silently resetting
    // to 0 would replay the entire backlog without a signal, so warn first.
    match std::fs::read_to_string(path) {
        Ok(s) => match s.trim().parse() {
            Ok(v) => Some(v),
            Err(_) => {
                eprintln!(
                    "bwoc triage: warning — malformed cursor {} ({:?}); restarting from 0",
                    path.display(),
                    s.trim()
                );
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!(
                "bwoc triage: warning — cannot read cursor {} ({e}); restarting from 0",
                path.display()
            );
            None
        }
    }
}

fn save_cursor(path: &Path, value: u64) -> Result<(), TriageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, value.to_string())?;
    Ok(())
}

/// Read every complete (newline-terminated) JSON envelope at or after byte
/// `start`. Returns `(bytes_consumed, envelopes)` — a partially-flushed trailing
/// line is left for the next pass. A missing inbox yields `(0, [])`.
fn read_envelopes_from(
    path: &Path,
    start: u64,
) -> Result<(u64, Vec<serde_json::Value>), TriageError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, Vec::new())),
        Err(e) => return Err(e.into()),
    };
    file.seek(SeekFrom::Start(start))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let mut consumed: u64 = 0;
    let mut out = Vec::new();
    for line in buf.split_inclusive('\n') {
        if !line.ends_with('\n') {
            break; // partial — wait for the rest
        }
        consumed += line.len() as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => out.push(v),
            Err(e) => eprintln!("bwoc triage: warning — skipped malformed envelope ({e})"),
        }
    }
    Ok((consumed, out))
}

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
    use bwoc_core::workspace::{AgentEntry, Workspace, WorkspaceDefaults, WorkspaceMeta};
    use std::fs;

    fn setup(label: &str) -> (PathBuf, AgentsRegistry) {
        let root = std::env::temp_dir().join(format!("bwoc-triage-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("agents/agent-orch/.bwoc")).unwrap();
        fs::create_dir_all(root.join("agents/agent-worker/.bwoc")).unwrap();
        Workspace {
            workspace: WorkspaceMeta {
                name: label.into(),
                version: "0.1.0".into(),
                created: "2026-01-01T00:00:00Z".into(),
            },
            defaults: WorkspaceDefaults::default(),
        }
        .save(&root)
        .unwrap();
        let mut reg = AgentsRegistry::default();
        for id in ["agent-orch", "agent-worker"] {
            reg.agents.push(AgentEntry {
                id: id.into(),
                path: format!("agents/{id}"),
                backend: "claude".into(),
                incarnated: "2026-01-01T00:00:00Z".into(),
                status: "active".into(),
            });
        }
        reg.save(&root).unwrap();
        (root, reg)
    }

    fn write_inbox(root: &Path, agent: &str, lines: &[&str]) {
        let p = root.join(format!("agents/{agent}/.bwoc/inbox.jsonl"));
        fs::write(p, format!("{}\n", lines.join("\n"))).unwrap();
    }

    #[test]
    fn classify_default_escalates_and_rules_win() {
        let mut cfg = Config::builtin();
        assert_eq!(cfg.classify("agent-x", "anything"), Action::Escalate);
        cfg.rules.push(Rule {
            pattern: "FYI".into(),
            action: Action::Ack,
        });
        cfg.rules.push(Rule {
            pattern: "from:agent-bot".into(),
            action: Action::Forward("agent-worker".into()),
        });
        assert_eq!(cfg.classify("agent-x", "FYI nothing to do"), Action::Ack);
        assert_eq!(
            cfg.classify("agent-bot", "please handle"),
            Action::Forward("agent-worker".into())
        );
        // First match wins; unmatched falls to default.
        assert_eq!(cfg.classify("agent-y", "hello"), Action::Escalate);
    }

    #[test]
    fn drain_advances_cursor_and_writes_receipts() {
        let (root, reg) = setup("drain");
        write_inbox(
            &root,
            "agent-orch",
            &[
                r#"{"ts":"t1","from":"agent-a","message":"coordinate please"}"#,
                r#"{"ts":"t2","from":"agent-b","message":"FYI heads up"}"#,
            ],
        );
        let entry = reg.agents.iter().find(|a| a.id == "agent-orch").unwrap();
        let inbox = entry.inbox_path(&root);
        let agent_dir = entry.dir(&root);
        let cursor_path = agent_dir.join(".bwoc/inbox.triage.cursor");
        let receipts = agent_dir.join(".bwoc/inbox.triage.jsonl");
        let cfg = Config::builtin();

        let new_cursor = drain(
            "agent-orch",
            &inbox,
            &cursor_path,
            &receipts,
            &root,
            &reg,
            &cfg,
            0,
            true,
            false,
        )
        .unwrap();
        // Cursor advanced to EOF; receipts written for both; re-draining is a no-op.
        assert_eq!(new_cursor, fs::metadata(&inbox).unwrap().len());
        let recs = fs::read_to_string(&receipts).unwrap();
        assert_eq!(recs.lines().filter(|l| !l.trim().is_empty()).count(), 2);
        let again = drain(
            "agent-orch",
            &inbox,
            &cursor_path,
            &receipts,
            &root,
            &reg,
            &cfg,
            new_cursor,
            true,
            false,
        )
        .unwrap();
        assert_eq!(again, new_cursor, "no reprocessing past the cursor");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn forward_redelivers_to_target_inbox() {
        let (root, reg) = setup("forward");
        write_inbox(
            &root,
            "agent-orch",
            &[r#"{"ts":"t1","from":"agent-a","message":"deploy now"}"#],
        );
        let entry = reg.agents.iter().find(|a| a.id == "agent-orch").unwrap();
        let inbox = entry.inbox_path(&root);
        let agent_dir = entry.dir(&root);
        let mut cfg = Config::builtin();
        cfg.rules.push(Rule {
            pattern: "deploy".into(),
            action: Action::Forward("agent-worker".into()),
        });
        drain(
            "agent-orch",
            &inbox,
            &agent_dir.join(".bwoc/inbox.triage.cursor"),
            &agent_dir.join(".bwoc/inbox.triage.jsonl"),
            &root,
            &reg,
            &cfg,
            0,
            true,
            false,
        )
        .unwrap();
        let worker_inbox = root.join("agents/agent-worker/.bwoc/inbox.jsonl");
        let delivered = fs::read_to_string(&worker_inbox).unwrap();
        assert!(delivered.contains("deploy now"));
        assert!(delivered.contains("forwarded_by"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_does_not_advance_cursor_or_write() {
        let (root, reg) = setup("dryrun");
        write_inbox(
            &root,
            "agent-orch",
            &[r#"{"ts":"t1","from":"agent-a","message":"x"}"#],
        );
        let entry = reg.agents.iter().find(|a| a.id == "agent-orch").unwrap();
        let agent_dir = entry.dir(&root);
        let cursor_path = agent_dir.join(".bwoc/inbox.triage.cursor");
        let receipts = agent_dir.join(".bwoc/inbox.triage.jsonl");
        drain(
            "agent-orch",
            &entry.inbox_path(&root),
            &cursor_path,
            &receipts,
            &root,
            &reg,
            &Config::builtin(),
            0,
            false,
            true, // dry_run
        )
        .unwrap();
        assert!(!cursor_path.exists(), "dry-run must not persist a cursor");
        assert!(!receipts.exists(), "dry-run must not write receipts");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn forward_to_missing_target_escalates_not_drops() {
        let (root, reg) = setup("badtarget");
        write_inbox(
            &root,
            "agent-orch",
            &[r#"{"ts":"t1","from":"agent-a","message":"deploy"}"#],
        );
        let entry = reg.agents.iter().find(|a| a.id == "agent-orch").unwrap();
        let agent_dir = entry.dir(&root);
        let receipts = agent_dir.join(".bwoc/inbox.triage.jsonl");
        let mut cfg = Config::builtin();
        cfg.rules.push(Rule {
            pattern: "deploy".into(),
            action: Action::Forward("agent-ghost".into()),
        });
        drain(
            "agent-orch",
            &entry.inbox_path(&root),
            &agent_dir.join(".bwoc/inbox.triage.cursor"),
            &receipts,
            &root,
            &reg,
            &cfg,
            0,
            true,
            false,
        )
        .unwrap();
        // A forward to a non-existent target must NOT silently drop the message:
        // it is recorded as escalate, not forward.
        let recs = fs::read_to_string(&receipts).unwrap();
        assert!(recs.contains(r#""action":"escalate""#));
        assert!(!recs.contains(r#""action":"forward""#));
        let _ = fs::remove_dir_all(&root);
    }
}
