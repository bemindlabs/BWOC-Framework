//! `bwoc outbox` — inspect and flush the sender-side durable retry queue.
//!
//! When `bwoc send` can't reach an offline remote peer it spools the signed
//! envelope (see `bwoc_core::outbox`). `bwoc outbox list` shows what's pending;
//! `bwoc outbox flush` retries delivery, replaying each stored envelope verbatim
//! (same `messageId` + signature → the recipient's inbox dedup makes retry
//! effectively-once). Delivered messages are dropped from the spool; ones whose
//! peer is still offline stay queued for the next flush.

use std::path::PathBuf;

use bwoc_core::workspace::AgentsRegistry;

use crate::send;

pub struct OutboxArgs {
    pub cmd: OutboxCmd,
    pub workspace: Option<PathBuf>,
}

pub enum OutboxCmd {
    /// Show pending counts per peer (the default).
    List,
    /// Retry delivery; `peer` limits it to one recipient.
    Flush { peer: Option<String> },
}

pub fn run(args: OutboxArgs) -> i32 {
    let Some(workspace) = crate::chat::resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc outbox: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    match args.cmd {
        OutboxCmd::List => list(&workspace),
        OutboxCmd::Flush { peer } => flush(&workspace, peer),
    }
}

fn list(workspace: &std::path::Path) -> i32 {
    match bwoc_core::outbox::list_pending(workspace) {
        Ok(pending) if pending.is_empty() => {
            println!("Outbox empty — nothing queued.");
            0
        }
        Ok(pending) => {
            let total: usize = pending.iter().map(|(_, n)| n).sum();
            println!(
                "Outbox: {total} message(s) queued for {} peer(s) — `bwoc outbox flush` to retry:",
                pending.len()
            );
            for (peer, n) in pending {
                println!("  {peer:<28} {n} pending");
            }
            0
        }
        Err(e) => {
            eprintln!("bwoc outbox: {e}");
            1
        }
    }
}

fn flush(workspace: &std::path::Path, peer: Option<String>) -> i32 {
    let registry = match AgentsRegistry::load(workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc outbox: failed to read agents registry: {e}");
            return 1;
        }
    };

    // Which peers' spools to drain: one (canonicalized) or all with pending.
    let peers: Vec<String> = match peer {
        Some(p) => vec![if p.starts_with("agent-") {
            p
        } else {
            format!("agent-{p}")
        }],
        None => match bwoc_core::outbox::list_pending(workspace) {
            Ok(pending) => pending.into_iter().map(|(id, _)| id).collect(),
            Err(e) => {
                eprintln!("bwoc outbox: {e}");
                return 1;
            }
        },
    };
    if peers.is_empty() {
        println!("Outbox empty — nothing to flush.");
        return 0;
    }

    let (mut delivered, mut still, mut hard) = (0usize, 0usize, 0usize);
    for peer_id in &peers {
        let lines = match bwoc_core::outbox::read_spooled(workspace, peer_id) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("  {peer_id}: cannot read spool: {e}");
                hard += 1;
                continue;
            }
        };
        if lines.is_empty() {
            continue;
        }
        let mut remaining = Vec::new();
        let (mut peer_ok, mut peer_left) = (0usize, 0usize);
        for line in lines {
            match send::redeliver(workspace, &registry, &line) {
                Ok(_) => {
                    delivered += 1;
                    peer_ok += 1;
                }
                // Peer still offline — keep it queued for the next flush.
                Err(e) if send::is_spoolable(&e) => {
                    remaining.push(line);
                    still += 1;
                    peer_left += 1;
                }
                // A hard error (stale route, unsigned gateway, …): keep the line
                // so nothing is lost, but surface it — the operator must act.
                Err(e) => {
                    remaining.push(line);
                    hard += 1;
                    peer_left += 1;
                    eprintln!("  {peer_id}: {e}");
                }
            }
        }
        if let Err(e) = bwoc_core::outbox::rewrite(workspace, peer_id, &remaining) {
            eprintln!("  {peer_id}: failed to update spool: {e}");
            hard += 1;
        }
        println!("  {peer_id:<28} {peer_ok} delivered, {peer_left} still queued");
    }

    println!();
    println!("Flushed: {delivered} delivered, {still} still queued (offline).");
    if hard > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bwoc_core::workspace::{
        AgentEntry, AgentsRegistry, Workspace, WorkspaceDefaults, WorkspaceMeta,
    };

    fn setup(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("bwoc-outboxcmd-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("agents/agent-beta/.bwoc")).unwrap();
        Workspace {
            workspace: WorkspaceMeta {
                name: label.to_string(),
                version: "0.1.0".to_string(),
                created: "2026-08-07T00:00:00Z".to_string(),
            },
            defaults: WorkspaceDefaults::default(),
        }
        .save(&root)
        .unwrap();
        let mut reg = AgentsRegistry::default();
        reg.agents.push(AgentEntry {
            id: "agent-beta".into(),
            path: "agents/agent-beta".into(),
            backend: "claude".into(),
            incarnated: "2026-08-07T00:00:00Z".into(),
            status: "active".into(),
        });
        reg.save(&root).unwrap();
        root
    }

    #[test]
    fn flush_delivers_a_local_target_and_drains_the_spool() {
        let root = setup("flush");
        let line = serde_json::json!({
            "ts": "2026-08-07T00:00:00Z",
            "messageId": "m1",
            "from": "user",
            "to": "agent-beta",
            "message": "queued while offline",
        })
        .to_string();
        bwoc_core::outbox::spool(&root, "agent-beta", "m1", &line).unwrap();

        let code = flush(&root, None);
        assert_eq!(code, 0);
        let inbox =
            std::fs::read_to_string(root.join("agents/agent-beta/.bwoc/inbox.jsonl")).unwrap();
        assert!(inbox.contains("queued while offline"));
        assert!(
            bwoc_core::outbox::read_spooled(&root, "agent-beta")
                .unwrap()
                .is_empty(),
            "spool drained after successful delivery"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn flush_keeps_a_poison_line_and_still_drains_the_good_one() {
        // A corrupt spool entry (unparseable JSON) must not wedge the flush: the
        // good line still delivers and drains, the poison line is kept (not lost,
        // not silently dropped) for an operator to inspect, and the run reports a
        // non-zero exit so the failure is visible.
        let root = setup("poison");
        let good = serde_json::json!({
            "ts": "2026-08-07T00:00:00Z",
            "messageId": "ok1",
            "from": "user",
            "to": "agent-beta",
            "message": "good one",
        })
        .to_string();
        // Order matters: poison first, good second — the good line must deliver
        // even though an earlier entry failed to parse.
        bwoc_core::outbox::spool(&root, "agent-beta", "bad1", "}{ not json at all").unwrap();
        bwoc_core::outbox::spool(&root, "agent-beta", "ok1", &good).unwrap();

        let code = flush(&root, None);
        assert_eq!(code, 1, "a hard (parse) error yields a non-zero exit");

        let inbox =
            std::fs::read_to_string(root.join("agents/agent-beta/.bwoc/inbox.jsonl")).unwrap();
        assert!(
            inbox.contains("good one"),
            "the parseable line still delivered"
        );

        let remaining = bwoc_core::outbox::read_spooled(&root, "agent-beta").unwrap();
        assert_eq!(remaining.len(), 1, "only the poison line is kept");
        assert!(
            remaining[0].contains("not json"),
            "the kept line is the poison entry, not the delivered one"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
