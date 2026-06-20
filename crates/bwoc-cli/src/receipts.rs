//! `bwoc receipts` — the read-receipt query (#299 read-receipt half).
//!
//! `bwoc send` dedups by `messageId` and reports a *delivery* receipt
//! (Delivered / Duplicate) at write time. This is the other half: a sender can
//! ask **"was my message actually consumed?"** A recipient's coordinator
//! (`bwoc triage`, #296) records a receipt for every envelope it processes to
//! `<agent>/.bwoc/inbox.triage.jsonl`, now keyed by the source `messageId`. This
//! command scans those receipt logs across the workspace and answers, per
//! message / sender / recipient, whether (and how — ack / escalate / forward) it
//! was consumed.
//!
//! Read-only and fleet-wide, like `bwoc tasks`. Cross-workspace receipts (a
//! recipient on another machine acking back through the gateway) are a transport
//! follow-up — this covers the local-workspace inbox/triage path the issue cites.

use std::path::{Path, PathBuf};

use bwoc_core::workspace::AgentsRegistry;

pub struct ReceiptsArgs {
    pub workspace: Option<PathBuf>,
    /// Filter to one source message id (the `bwoc send` `[id …]`).
    pub message_id: Option<String>,
    /// Filter to receipts for messages from this sender (literal — `user`,
    /// `agent-x`, …; a bare name also matches `agent-<name>`).
    pub from: Option<String>,
    /// Filter to one recipient agent (the consumer; bare name auto-prefixed).
    pub agent: Option<String>,
    pub json: bool,
}

/// One consumption record, flattened from a recipient's triage receipt log.
struct Receipt {
    recipient: String,
    message_id: Option<String>,
    from: String,
    action: String,
    ts: String,
    message: String,
}

pub fn run(args: ReceiptsArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc receipts: no workspace (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc receipts: cannot load agent registry: {e}");
            return 2;
        }
    };

    let from_filter = args.from.as_deref();
    let agent_filter = args.agent.as_deref().map(normalize_agent);
    let id_filter = args.message_id.as_deref();

    let mut rows: Vec<Receipt> = Vec::new();
    for entry in &registry.agents {
        let recipient = entry.id.clone();
        if let Some(ag) = &agent_filter {
            if &recipient != ag {
                continue;
            }
        }
        let path = entry
            .dir(&workspace)
            .join(".bwoc")
            .join("inbox.triage.jsonl");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue; // no receipts for this agent yet
        };
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue; // a malformed receipt line — skip, don't abort the scan
            };
            let message_id = v.get("messageId").and_then(|x| x.as_str());
            if let Some(want) = id_filter {
                if message_id != Some(want) {
                    continue;
                }
            }
            let from = v.get("from").and_then(|x| x.as_str()).unwrap_or("—");
            if let Some(want) = from_filter {
                if from != want && format!("agent-{want}") != from {
                    continue;
                }
            }
            rows.push(Receipt {
                recipient: recipient.clone(),
                message_id: message_id.map(str::to_string),
                from: from.to_string(),
                action: v
                    .get("action")
                    .and_then(|x| x.as_str())
                    .unwrap_or("—")
                    .to_string(),
                ts: v
                    .get("ts")
                    .and_then(|x| x.as_str())
                    .unwrap_or("—")
                    .to_string(),
                message: v
                    .get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    // Stable order: recipient, then ts.
    rows.sort_by(|a, b| {
        (a.recipient.as_str(), a.ts.as_str()).cmp(&(b.recipient.as_str(), b.ts.as_str()))
    });

    if args.json {
        emit_json(&workspace, &rows);
    } else {
        print_table(&workspace, &rows, id_filter);
    }
    0
}

fn normalize_agent(a: &str) -> String {
    if a.starts_with("agent-") {
        a.to_string()
    } else {
        format!("agent-{a}")
    }
}

fn print_table(workspace: &Path, rows: &[Receipt], id_filter: Option<&str>) {
    println!();
    println!(
        "Read receipts — {} ({} receipt(s))",
        workspace.display(),
        rows.len()
    );
    println!();
    if rows.is_empty() {
        // A message-id query with no hit is the explicit "not consumed yet" case.
        if let Some(id) = id_filter {
            println!(
                "(no receipt for message `{id}` — not consumed yet, or the recipient has not run `bwoc triage`)"
            );
        } else {
            println!(
                "(no receipts — recipients record them when `bwoc triage` consumes a message)"
            );
        }
        println!();
        return;
    }
    let rcpt_w = rows
        .iter()
        .map(|r| r.recipient.len())
        .max()
        .unwrap_or(9)
        .max(9);
    let id_w = rows
        .iter()
        .map(|r| r.message_id.as_deref().unwrap_or("—").len())
        .max()
        .unwrap_or(10)
        .max(10);
    let from_w = rows.iter().map(|r| r.from.len()).max().unwrap_or(4).max(4);
    println!(
        "  {:<rcpt_w$}  {:<id_w$}  {:<from_w$}  {:<9}  TS",
        "RECIPIENT", "MESSAGE-ID", "FROM", "ACTION"
    );
    for r in rows {
        println!(
            "  {:<rcpt_w$}  {:<id_w$}  {:<from_w$}  {:<9}  {}",
            r.recipient,
            r.message_id.as_deref().unwrap_or("—"),
            r.from,
            r.action,
            r.ts,
        );
    }
    println!();
}

fn emit_json(workspace: &Path, rows: &[Receipt]) {
    let receipts: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "recipient": r.recipient,
                "messageId": r.message_id,
                "from": r.from,
                "action": r.action,
                "ts": r.ts,
                "message": r.message,
            })
        })
        .collect();
    let value = serde_json::json!({
        "workspace": workspace.display().to_string(),
        "receipts": receipts,
    });
    match serde_json::to_string_pretty(&value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("bwoc receipts: failed to serialize: {e}"),
    }
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
    use bwoc_core::workspace::{AgentEntry, AgentsRegistry};
    use std::fs;

    fn setup(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("bwoc-receipts-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        fs::write(
            root.join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"t\"\nversion=\"0.1.0\"\ncreated=\"2026-01-01T00:00:00Z\"\n",
        )
        .unwrap();
        root
    }

    /// Register one agent and write its triage receipt log.
    fn agent_with_receipts(root: &Path, id: &str, lines: &[String]) {
        let dir = root.join("agents").join(id);
        fs::create_dir_all(dir.join(".bwoc")).unwrap();
        fs::write(
            dir.join(".bwoc/inbox.triage.jsonl"),
            format!("{}\n", lines.join("\n")),
        )
        .unwrap();
        let mut reg = AgentsRegistry::load(root).unwrap_or_default();
        reg.agents.push(AgentEntry {
            id: id.to_string(),
            path: format!("agents/{id}"),
            backend: "cli".to_string(),
            incarnated: "2026-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
        });
        reg.save(root).unwrap();
    }

    fn receipt(mid: &str, from: &str, action: &str, ts: &str) -> String {
        serde_json::json!({
            "agent": "agent-x", "ts": ts, "from": from,
            "action": action, "message": "hi", "messageId": mid,
        })
        .to_string()
    }

    #[test]
    fn message_id_filter_finds_the_consuming_receipt() {
        let root = setup("byid");
        agent_with_receipts(
            &root,
            "agent-pi",
            &[
                receipt("msg-1", "user", "ack", "t1"),
                receipt("msg-2", "agent-a", "forward", "t2"),
            ],
        );
        assert_eq!(
            run(ReceiptsArgs {
                workspace: Some(root.clone()),
                message_id: Some("msg-2".into()),
                from: None,
                agent: None,
                json: true,
            }),
            0
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn message_not_found_is_exit_zero_not_an_error() {
        // "Not consumed yet" is a successful query (exit 0, empty) — the sender
        // learns the message has no receipt, which is the whole point.
        let root = setup("notfound");
        agent_with_receipts(&root, "agent-pi", &[receipt("msg-1", "user", "ack", "t1")]);
        assert_eq!(
            run(ReceiptsArgs {
                workspace: Some(root.clone()),
                message_id: Some("msg-never-sent".into()),
                from: None,
                agent: None,
                json: false,
            }),
            0
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn from_filter_matches_bare_and_prefixed() {
        let root = setup("from");
        agent_with_receipts(
            &root,
            "agent-pi",
            &[
                receipt("m1", "agent-busaba", "ack", "t1"),
                receipt("m2", "user", "escalate", "t2"),
            ],
        );
        // `--from busaba` should match `agent-busaba` via the bare-name fallback.
        assert_eq!(
            run(ReceiptsArgs {
                workspace: Some(root.clone()),
                message_id: None,
                from: Some("busaba".into()),
                agent: None,
                json: true,
            }),
            0
        );
        let _ = fs::remove_dir_all(&root);
    }
}
