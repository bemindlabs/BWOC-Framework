//! `bwoc inbox <agent>` — read companion to `bwoc send`.
//!
//! Reads `<agent>/.bwoc/inbox.jsonl` (JSON-lines format spec'd by
//! `send.rs`), parses each line as one envelope, and prints them in
//! send order. Malformed lines emit a warning to stderr but don't
//! abort — the rest of the inbox is still readable.

use std::path::PathBuf;

use bwoc_core::workspace::AgentsRegistry;

pub struct InboxArgs {
    /// Empty when `--all`. The CLI shim enforces "exactly one of agent | all".
    pub agent: String,
    pub workspace: Option<PathBuf>,
    pub json: bool,
    pub limit: Option<usize>,
    /// Tail mode: print historical messages then block, printing new
    /// envelopes as they arrive. Exit with Ctrl-C.
    pub watch: bool,
    /// Truncate the inbox after printing (acknowledge messages). TTY
    /// prompts unless `yes` is also set.
    pub clear: bool,
    /// Skip the interactive confirmation for `clear`. Required for non-TTY.
    pub yes: bool,
    /// Print just the message count (one integer) instead of envelopes.
    /// Useful for shell scripts:
    ///   `if [ $(bwoc inbox alpha --count) -gt 0 ]; then ...`
    /// With `--json`, emits `{"count": N}`.
    pub count: bool,
    /// Print every agent's inbox concatenated, each preceded by a
    /// `=== <agent-id> (N message(s)) ===` header. Mutually exclusive
    /// with `agent`. With `--watch`, becomes the merged live tail (issue
    /// #46): every inbox interleaved in arrival order, each envelope
    /// tagged with its recipient. `--clear` is still refused with `--all`
    /// (mass-clear is too destructive — clear one agent at a time).
    pub all: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum InboxError {
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
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn run(args: InboxArgs) -> i32 {
    match inbox(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc inbox: {e}");
            match e {
                InboxError::NoWorkspace | InboxError::NotFound { .. } => 2,
                _ => 1,
            }
        }
    }
}

fn inbox(args: InboxArgs) -> Result<(), InboxError> {
    let workspace = resolve_workspace(args.workspace).ok_or(InboxError::NoWorkspace)?;
    let registry = AgentsRegistry::load(&workspace)?;

    if args.all {
        if let Some(msg) = all_flag_refusal(args.clear) {
            eprintln!("{msg}");
            return Ok(());
        }
        if args.watch {
            // Merged live tail across every agent's inbox (issue #46) — a
            // fleet-wide stream, not a new watcher: it reuses the same
            // per-file poll mechanism as the single-inbox `--watch`.
            return watch_inbox_all(&workspace, &registry, args.json, args.limit);
        }
        return inbox_all(&workspace, &registry, args.json, args.limit);
    }

    let lookup_id = if args.agent.starts_with("agent-") {
        args.agent.clone()
    } else {
        format!("agent-{}", args.agent)
    };
    let entry = registry
        .agents
        .iter()
        .find(|a| a.id == lookup_id)
        .ok_or_else(|| InboxError::NotFound {
            name: args.agent.clone(),
            workspace: workspace.clone(),
        })?;

    let inbox_path = workspace.join(&entry.path).join(".bwoc/inbox.jsonl");
    let messages = read_messages(&inbox_path)?;

    // --count short-circuit: just the integer, before --limit/--json
    // shape any view. Filters are no-ops on a count anyway.
    if args.count {
        if args.json {
            let value = serde_json::json!({ "count": messages.len() });
            println!("{}", serde_json::to_string(&value)?);
        } else {
            println!("{}", messages.len());
        }
        return Ok(());
    }

    // Apply --limit (last N).
    let view: Vec<&serde_json::Value> = if let Some(n) = args.limit {
        messages.iter().rev().take(n).rev().collect()
    } else {
        messages.iter().collect()
    };

    if args.json && !args.watch {
        // Snapshot mode — pretty-printed full envelope set.
        let value = serde_json::json!({
            "agent": entry.id,
            "inbox": inbox_path.display().to_string(),
            "total": messages.len(),
            "shown": view.len(),
            "messages": view,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    if args.json && args.watch {
        // Streaming mode — emit historical envelopes (last `view`) as
        // compact JSON lines, then block tailing for new ones (also
        // emitted as JSON lines). Consumers can `jq -c '.'` per line.
        for m in &view {
            println!("{}", serde_json::to_string(m)?);
        }
        return watch_inbox_json(&inbox_path);
    }

    println!();
    println!("Inbox for {}: {} message(s)", entry.id, messages.len());
    if let Some(n) = args.limit {
        if view.len() < messages.len() {
            println!("(showing last {} of {})", view.len(), messages.len());
        } else {
            let _ = n; // limit larger than count; suppress redundant note
        }
    }
    println!("Source: {}", inbox_path.display());
    println!();
    if view.is_empty() {
        println!("(no messages)");
        println!();
        return Ok(());
    }
    for (i, m) in view.iter().enumerate() {
        print_envelope(i + 1, m);
    }

    // --clear is incompatible with --watch (one drains, the other listens
    // forever — combining them is almost certainly a mistake). Reject
    // explicitly so the user picks one.
    if args.clear && args.watch {
        eprintln!("bwoc inbox: --clear and --watch can't be combined (clear drains; watch waits)");
        return Ok(());
    }

    if args.clear {
        clear_inbox(&inbox_path, messages.len(), args.yes)?;
    }

    if args.watch {
        watch_inbox(&inbox_path, messages.len())?;
    }
    Ok(())
}

/// Truncate the inbox file after the user confirms. The daemon's
/// `check_inbox_for_new` already handles file truncation gracefully —
/// it resets its cursor on shrink, so a live daemon won't crash or
/// re-announce pre-clear messages.
fn clear_inbox(inbox_path: &std::path::Path, count: usize, yes: bool) -> Result<(), InboxError> {
    use std::io::{IsTerminal, Write};

    if count == 0 {
        println!("(nothing to clear)");
        return Ok(());
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("bwoc inbox --clear: non-TTY without --yes — aborted");
            return Ok(());
        }
        print!(
            "Delete {count} message(s) from {}? [y/N]: ",
            inbox_path.display()
        );
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            println!("(aborted — nothing changed)");
            return Ok(());
        }
    }
    // Truncate to zero by re-opening with truncate=true. Simpler than
    // remove+create — preserves the file inode so a watching daemon's
    // open handle (if any) stays valid in the truncation case.
    std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(inbox_path)?;
    println!("Cleared {count} message(s) from {}.", inbox_path.display());
    Ok(())
}

/// Print one envelope as a numbered block (one ts/from header, then
/// indented message body — multi-line aware).
fn print_envelope(idx: usize, m: &serde_json::Value) {
    let ts = m.get("ts").and_then(|v| v.as_str()).unwrap_or("—");
    let from = m.get("from").and_then(|v| v.as_str()).unwrap_or("—");
    let msg = m
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("(no message)");
    println!("  [{idx}]  {ts}  ←  {from}");
    for line in msg.lines() {
        println!("        {line}");
    }
    println!();
}

/// Tail mode — block reading new envelopes from `inbox_path` past the
/// last known count. Exit with Ctrl-C (default SIGINT terminates the
/// process; no graceful state to flush since this is read-only).
/// Print every agent's inbox concatenated. Per-agent header is
/// `=== <agent-id> (N message(s)) ===`. Empty inboxes still get a
/// header (so the user knows the agent was inspected — silent skipping
/// is more confusing than a "0 message(s)" line).
///
/// `--json` shape:
///   {
///     "workspace": "<path>",
///     "agents": [
///       { "agent": "agent-foo", "inbox": "<path>", "total": N,
///         "shown": M, "messages": [...] },
///       ...
///     ]
///   }
fn inbox_all(
    workspace: &std::path::Path,
    registry: &AgentsRegistry,
    json: bool,
    limit: Option<usize>,
) -> Result<(), InboxError> {
    if registry.agents.is_empty() {
        if json {
            let value = serde_json::json!({
                "workspace": workspace.display().to_string(),
                "agents": [],
            });
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!(
                "bwoc inbox --all: no agents registered in {}.",
                workspace.display()
            );
        }
        return Ok(());
    }

    if json {
        let mut per_agent: Vec<serde_json::Value> = Vec::with_capacity(registry.agents.len());
        for entry in &registry.agents {
            let inbox_path = workspace.join(&entry.path).join(".bwoc/inbox.jsonl");
            let messages = read_messages(&inbox_path)?;
            let view: Vec<&serde_json::Value> = if let Some(n) = limit {
                messages.iter().rev().take(n).rev().collect()
            } else {
                messages.iter().collect()
            };
            per_agent.push(serde_json::json!({
                "agent": entry.id,
                "inbox": inbox_path.display().to_string(),
                "total": messages.len(),
                "shown": view.len(),
                "messages": view,
            }));
        }
        let value = serde_json::json!({
            "workspace": workspace.display().to_string(),
            "agents": per_agent,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    // Human mode. One section per agent.
    for entry in &registry.agents {
        let inbox_path = workspace.join(&entry.path).join(".bwoc/inbox.jsonl");
        let messages = read_messages(&inbox_path)?;
        let view: Vec<&serde_json::Value> = if let Some(n) = limit {
            messages.iter().rev().take(n).rev().collect()
        } else {
            messages.iter().collect()
        };
        println!();
        println!("=== {} ({} message(s)) ===", entry.id, messages.len());
        if messages.is_empty() {
            continue;
        }
        for v in &view {
            let ts = v.get("ts").and_then(|x| x.as_str()).unwrap_or("?");
            let from = v.get("from").and_then(|x| x.as_str()).unwrap_or("?");
            let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("?");
            println!("[{ts}] {from}: {msg}");
        }
    }
    println!();
    Ok(())
}

/// One parsed line from an inbox tail: either a decoded envelope, or a
/// malformed line carried as `(parse-error, raw-text)` so the caller can
/// decide how to surface it (stderr warning vs. JSON error envelope).
type LineResult = Result<serde_json::Value, (String, String)>;

/// Read the complete (newline-terminated) lines appended to `path` at or
/// after byte `start`. Returns `(bytes_consumed, lines)` — the caller owns
/// offset bookkeeping, truncation policy, and poll cadence; this owns the
/// fiddly part: seek, split on whole lines only, parse, and count exactly
/// the bytes consumed so a partially-flushed trailing line is retried next
/// tick. A missing file yields `(0, [])` (an inbox no one has written to
/// yet is just empty, not an error). This is the single tail mechanism
/// shared by the single-inbox `--watch`, its `--json` variant, and the
/// merged `--all --watch` stream.
fn read_complete_lines_from(
    path: &std::path::Path,
    start: u64,
) -> Result<(u64, Vec<LineResult>), InboxError> {
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
    let mut out: Vec<LineResult> = Vec::new();
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
            Ok(v) => out.push(Ok(v)),
            Err(e) => out.push(Err((e.to_string(), trimmed.to_string()))),
        }
    }
    Ok((consumed, out))
}

/// JSON streaming tail. Same poll loop as `watch_inbox`, but each new
/// envelope is printed as a single compact-JSON line — no header, no
/// numbering, no decoration. Designed for `bwoc inbox alpha --watch
/// --json | jq -c '.'` or piping into a log aggregator.
fn watch_inbox_json(inbox_path: &std::path::Path) -> Result<(), InboxError> {
    use std::time::Duration;
    let mut offset: u64 = std::fs::metadata(inbox_path).map(|m| m.len()).unwrap_or(0);
    loop {
        let Ok(meta) = std::fs::metadata(inbox_path) else {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        let size = meta.len();
        if size < offset {
            offset = size;
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        if size == offset {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        let (consumed, lines) = read_complete_lines_from(inbox_path, offset)?;
        for line in lines {
            match line {
                // Pass through verbatim. On a malformed line emit a
                // one-shot error envelope so consumers see something
                // rather than silently dropping it.
                Ok(v) => println!("{}", serde_json::to_string(&v)?),
                Err((detail, raw)) => {
                    let err = serde_json::json!({
                        "error": "malformed_envelope",
                        "detail": detail,
                        "raw": raw,
                    });
                    println!("{}", serde_json::to_string(&err)?);
                }
            }
        }
        offset += consumed;
    }
}

fn watch_inbox(inbox_path: &std::path::Path, mut idx: usize) -> Result<(), InboxError> {
    use std::time::Duration;

    println!("(watching for new messages — Ctrl-C to stop)");
    println!();

    // Track byte offset rather than line count — robust to lines being
    // partially flushed. Starts at current EOF so we don't re-print the
    // historical view.
    let mut offset: u64 = std::fs::metadata(inbox_path).map(|m| m.len()).unwrap_or(0);

    loop {
        let Ok(meta) = std::fs::metadata(inbox_path) else {
            // File might be missing if no one has sent yet — keep polling.
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        let size = meta.len();
        if size < offset {
            // Truncation — reset.
            eprintln!("(inbox truncated; resuming from new EOF)");
            offset = size;
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        if size == offset {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        let (consumed, lines) = read_complete_lines_from(inbox_path, offset)?;
        for line in lines {
            match line {
                Ok(v) => {
                    idx += 1;
                    print_envelope(idx, &v);
                }
                Err((e, _raw)) => {
                    eprintln!("bwoc inbox: warning — malformed JSON skipped ({e})");
                }
            }
        }
        offset += consumed;
    }
}

/// With `--all`, `--clear` stays refused — a mass-clear across every agent
/// is too destructive to express in one flag (clear one inbox at a time).
/// `--watch`, by contrast, is now the merged live tail (issue #46), so this
/// predicate deliberately does NOT consider `watch`: `--all --watch` is a
/// supported combination. Returns the refusal message when disallowed.
fn all_flag_refusal(clear: bool) -> Option<&'static str> {
    if clear {
        Some(
            "bwoc inbox --all: --clear is not supported with --all (mass-clear is too \
             destructive — use `bwoc inbox <name> --clear` per agent, or \
             `bwoc list --inbox-pending` to find candidates).",
        )
    } else {
        None
    }
}

/// Return a copy of `env` with a top-level `recipient` field naming the
/// agent whose inbox it came from — the tag that lets a merged
/// `--all --watch` stream stay attributable. A well-formed envelope is a
/// JSON object; anything else is wrapped rather than dropped.
fn tag_with_recipient(recipient: &str, env: &serde_json::Value) -> serde_json::Value {
    let mut v = env.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "recipient".to_string(),
            serde_json::Value::String(recipient.to_string()),
        );
        v
    } else {
        serde_json::json!({ "recipient": recipient, "envelope": env })
    }
}

/// Human-mode line for the merged tail: a header naming the recipient and
/// sender, then the (multi-line-aware) message body indented beneath. No
/// global numbering — index has no meaning across interleaved inboxes.
fn print_tagged_envelope(recipient: &str, m: &serde_json::Value) {
    let ts = m.get("ts").and_then(|v| v.as_str()).unwrap_or("—");
    let from = m.get("from").and_then(|v| v.as_str()).unwrap_or("—");
    let msg = m
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("(no message)");
    println!("  {ts}  →{recipient}  ←{from}");
    for line in msg.lines() {
        println!("        {line}");
    }
    println!();
}

/// Emit one envelope in the merged stream, tagged with its recipient —
/// compact JSON line (`--json`) or the human block above.
fn emit_tagged(recipient: &str, m: &serde_json::Value, json: bool) -> Result<(), InboxError> {
    if json {
        println!(
            "{}",
            serde_json::to_string(&tag_with_recipient(recipient, m))?
        );
    } else {
        print_tagged_envelope(recipient, m);
    }
    Ok(())
}

/// Merged live tail across every agent's inbox (issue #46). Reuses the
/// single-inbox poll mechanism (`read_complete_lines_from`) per file and
/// interleaves the outputs — no new watcher, no global sort. `--limit`
/// applies to the per-agent backlog printed before tailing begins; after
/// that, only newly appended envelopes are emitted, each tagged with its
/// recipient in arrival order (Samānattatā — every inbox watched equally).
/// A missing inbox is skipped, not an error. Blocks until Ctrl-C.
fn watch_inbox_all(
    workspace: &std::path::Path,
    registry: &AgentsRegistry,
    json: bool,
    limit: Option<usize>,
) -> Result<(), InboxError> {
    use std::time::Duration;

    struct Tail {
        recipient: String,
        path: PathBuf,
        offset: u64,
    }

    // Backlog first: print the last `limit` of each inbox (tagged), then
    // pin each tail's offset to that inbox's current EOF so the live loop
    // emits only envelopes that arrive from here on.
    let mut tails: Vec<Tail> = Vec::with_capacity(registry.agents.len());
    for entry in &registry.agents {
        let path = workspace.join(&entry.path).join(".bwoc/inbox.jsonl");
        let messages = read_messages(&path)?; // missing inbox -> empty (graceful)
        let view: Vec<&serde_json::Value> = if let Some(n) = limit {
            messages.iter().rev().take(n).rev().collect()
        } else {
            messages.iter().collect()
        };
        for m in &view {
            emit_tagged(&entry.id, m, json)?;
        }
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        tails.push(Tail {
            recipient: entry.id.clone(),
            path,
            offset,
        });
    }

    if !json {
        println!("(watching all inboxes for new messages — Ctrl-C to stop)");
        println!();
    }

    loop {
        let mut any = false;
        for t in &mut tails {
            // Per-file size/offset check mirrors the single-inbox loops;
            // a missing inbox just has no metadata this tick — skip it.
            let Ok(meta) = std::fs::metadata(&t.path) else {
                continue;
            };
            let size = meta.len();
            if size < t.offset {
                t.offset = size; // truncated — reset, stay quiet across the fleet
                continue;
            }
            if size == t.offset {
                continue;
            }
            let (consumed, lines) = read_complete_lines_from(&t.path, t.offset)?;
            for line in lines {
                any = true;
                match line {
                    Ok(v) => emit_tagged(&t.recipient, &v, json)?,
                    Err((detail, raw)) => {
                        if json {
                            let err = serde_json::json!({
                                "recipient": t.recipient,
                                "error": "malformed_envelope",
                                "detail": detail,
                                "raw": raw,
                            });
                            println!("{}", serde_json::to_string(&err)?);
                        } else {
                            eprintln!(
                                "bwoc inbox: warning — malformed JSON in {} skipped ({detail})",
                                t.recipient
                            );
                        }
                    }
                }
            }
            t.offset += consumed;
        }
        if !any {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

/// Read the inbox file line-by-line. Malformed lines warn-and-skip;
/// missing file is treated as an empty inbox. Any refusals recorded by
/// the daemon in the sibling `inbox.refusals.jsonl` are merged in: the
/// matching envelope gets a `refused: { reason, missing }` field so the
/// trust spec's `jq '.[] | select(.refused)'` example works against
/// either snapshot mode.
fn read_messages(path: &std::path::Path) -> Result<Vec<serde_json::Value>, InboxError> {
    let envelopes = read_envelopes_with_offsets(path)?;
    let refusals_path = path.with_file_name("inbox.refusals.jsonl");
    let refusals = read_refusals(&refusals_path);
    let mut out = Vec::with_capacity(envelopes.len());
    for (offset, mut env) in envelopes {
        if let Some(refused) = refusals.get(&offset) {
            if let Some(obj) = env.as_object_mut() {
                obj.insert("refused".to_string(), refused.clone());
            }
        }
        out.push(env);
    }
    Ok(out)
}

/// Read inbox.jsonl with byte-offset tracking per line. The offset is
/// the byte position of the first character of each line within the file
/// — the same key the daemon writes into refusal records.
fn read_envelopes_with_offsets(
    path: &std::path::Path,
) -> Result<Vec<(u64, serde_json::Value)>, InboxError> {
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let mut out = Vec::new();
    let mut offset: u64 = 0;
    let mut lineno: usize = 0;
    for line in buf.split_inclusive('\n') {
        lineno += 1;
        let line_offset = offset;
        offset += line.len() as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => out.push((line_offset, v)),
            Err(e) => {
                eprintln!("bwoc inbox: warning — line {lineno} is malformed JSON, skipped ({e})");
            }
        }
    }
    Ok(out)
}

/// Read the daemon's refusal sidecar into a map keyed by envelope offset.
/// Map value is the `{reason, missing}` view attached to the envelope
/// — `ts` and `envelope*` echo fields stay in the sidecar but don't
/// pollute the envelope display. Best-effort: missing or malformed file
/// returns an empty map; the inbox is still readable.
fn read_refusals(path: &std::path::Path) -> std::collections::HashMap<u64, serde_json::Value> {
    use std::collections::HashMap;
    let mut out: HashMap<u64, serde_json::Value> = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(offset) = v.get("envelopeOffset").and_then(|x| x.as_u64()) else {
            continue;
        };
        let reason = v
            .get("reason")
            .cloned()
            .unwrap_or(serde_json::Value::String("missing_trust".into()));
        let missing = v
            .get("missing")
            .cloned()
            .unwrap_or(serde_json::Value::Array(Vec::new()));
        out.insert(
            offset,
            serde_json::json!({ "reason": reason, "missing": missing }),
        );
    }
    out
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
    use bwoc_core::workspace::{
        AgentEntry, AgentsRegistry, Workspace, WorkspaceDefaults, WorkspaceMeta,
    };
    use std::fs;
    use std::io::Write;

    fn setup(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-inbox-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        fs::create_dir_all(root.join("agents/agent-alpha/.bwoc")).unwrap();
        Workspace {
            workspace: WorkspaceMeta {
                name: label.into(),
                version: "0.1.0".into(),
                created: "2026-05-22T00:00:00Z".into(),
            },
            defaults: WorkspaceDefaults::default(),
        }
        .save(&root)
        .unwrap();
        let mut reg = AgentsRegistry::default();
        reg.agents.push(AgentEntry {
            id: "agent-alpha".into(),
            path: "agents/agent-alpha".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22T00:00:00Z".into(),
            status: "active".into(),
        });
        reg.save(&root).unwrap();
        root
    }

    fn write_inbox(root: &std::path::Path, lines: &[&str]) {
        let p = root.join("agents/agent-alpha/.bwoc/inbox.jsonl");
        let mut f = fs::File::create(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn read_skips_malformed_lines_but_keeps_valid_ones() {
        let root = setup("malformed");
        write_inbox(
            &root,
            &[
                r#"{"ts":"t1","from":"user","to":"agent-alpha","message":"hello"}"#,
                "this is not json",
                r#"{"ts":"t2","from":"user","to":"agent-alpha","message":"world"}"#,
            ],
        );
        let msgs = read_messages(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl")).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["message"], "hello");
        assert_eq!(msgs[1]["message"], "world");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_inbox_is_empty_not_error() {
        let root = setup("missing");
        let msgs = read_messages(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl")).unwrap();
        assert!(msgs.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn limit_returns_last_n() {
        // Direct slice-test of the limit logic (the run loop does it
        // identically). 5 messages, limit 2 → last 2.
        let msgs: Vec<serde_json::Value> = (0..5)
            .map(|i| serde_json::json!({ "message": format!("m{i}") }))
            .collect();
        let view: Vec<&serde_json::Value> = msgs.iter().rev().take(2).rev().collect();
        assert_eq!(view.len(), 2);
        assert_eq!(view[0]["message"], "m3");
        assert_eq!(view[1]["message"], "m4");
    }

    // ---- Trust refusal merge (step 4) ---------------------------------------

    fn write_refusals(root: &std::path::Path, lines: &[&str]) {
        let p = root.join("agents/agent-alpha/.bwoc/inbox.refusals.jsonl");
        let mut f = fs::File::create(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn read_envelopes_offsets_match_line_starts() {
        let root = setup("offsets");
        write_inbox(
            &root,
            &[
                r#"{"ts":"t1","from":"user","to":"agent-alpha","message":"a"}"#,
                r#"{"ts":"t2","from":"agent-x","to":"agent-alpha","message":"b"}"#,
            ],
        );
        let pairs = read_envelopes_with_offsets(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl"))
            .unwrap();
        assert_eq!(pairs.len(), 2);
        // First line starts at byte 0.
        assert_eq!(pairs[0].0, 0);
        // Second line starts past the first (which is 58 bytes + newline = 59).
        let first_len = pairs[0].1.to_string().len();
        assert!(pairs[1].0 > 0);
        assert!(pairs[1].0 >= first_len as u64);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refusals_merge_into_envelopes_by_offset() {
        let root = setup("merge");
        // Two envelopes; daemon refused the second one.
        write_inbox(
            &root,
            &[
                r#"{"ts":"t1","from":"user","to":"agent-alpha","message":"ok"}"#,
                r#"{"ts":"t2","from":"agent-x","to":"agent-alpha","message":"no"}"#,
            ],
        );
        // Figure out where line 2 starts.
        let pairs = read_envelopes_with_offsets(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl"))
            .unwrap();
        let line2_offset = pairs[1].0;
        let refusal_line = format!(
            r#"{{"ts":"refusal-ts","envelopeOffset":{line2_offset},"envelopeTs":"t2","envelopeFrom":"agent-x","reason":"missing_trust","missing":["vatta"]}}"#
        );
        write_refusals(&root, &[&refusal_line]);

        let msgs = read_messages(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl")).unwrap();
        assert_eq!(msgs.len(), 2);
        // First envelope: no refusal attached.
        assert!(msgs[0].get("refused").is_none());
        // Second envelope: refused field with reason + missing list.
        let refused = msgs[1].get("refused").expect("expected refused field");
        assert_eq!(refused["reason"], "missing_trust");
        assert_eq!(refused["missing"], serde_json::json!(["vatta"]));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_refusals_file_is_silent_passthrough() {
        let root = setup("no-refusals");
        write_inbox(
            &root,
            &[r#"{"ts":"t1","from":"user","to":"agent-alpha","message":"x"}"#],
        );
        // No inbox.refusals.jsonl on disk — read_messages should not
        // attach any `refused` fields and not error.
        let msgs = read_messages(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl")).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].get("refused").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_refusal_line_is_skipped() {
        let root = setup("bad-refusal");
        write_inbox(
            &root,
            &[r#"{"ts":"t1","from":"agent-x","to":"agent-alpha","message":"x"}"#],
        );
        write_refusals(
            &root,
            &[
                "this is not json",
                r#"{"missing":"envelopeOffset-field"}"#, // legal JSON but no offset
            ],
        );
        let msgs = read_messages(&root.join("agents/agent-alpha/.bwoc/inbox.jsonl")).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].get("refused").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    // ---- Merged --all --watch (issue #46) -----------------------------------

    #[test]
    fn all_watch_no_longer_refused() {
        // The whole point of #46: `--all --watch` (clear=false) must NOT be
        // refused — the predicate ignores `watch`. `--clear` still is.
        assert!(
            all_flag_refusal(false).is_none(),
            "--all (with or without --watch) must be allowed"
        );
        assert!(
            all_flag_refusal(true).is_some(),
            "--all --clear must still be refused"
        );
    }

    #[test]
    fn merged_tail_tags_recipient() {
        let env = serde_json::json!({
            "ts": "t1", "from": "agent-x", "to": "agent-beta", "message": "hi"
        });
        let tagged = tag_with_recipient("agent-beta", &env);
        // Recipient added, original fields preserved.
        assert_eq!(tagged["recipient"], "agent-beta");
        assert_eq!(tagged["from"], "agent-x");
        assert_eq!(tagged["message"], "hi");
        // A non-object line is wrapped, not dropped.
        let wrapped = tag_with_recipient("agent-beta", &serde_json::json!("oops"));
        assert_eq!(wrapped["recipient"], "agent-beta");
        assert_eq!(wrapped["envelope"], "oops");
    }

    #[test]
    fn tail_skips_missing_inbox() {
        // A registered agent whose inbox file doesn't exist yet must yield
        // no lines and no error (graceful degrade), and not advance offset.
        let root = setup("tail-missing");
        let missing = root.join("agents/agent-alpha/.bwoc/inbox.jsonl");
        let (consumed, lines) = read_complete_lines_from(&missing, 0).unwrap();
        assert_eq!(consumed, 0);
        assert!(lines.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tail_reads_only_complete_appended_lines() {
        let root = setup("tail-append");
        let path = root.join("agents/agent-alpha/.bwoc/inbox.jsonl");
        // Two complete lines plus a partial (no trailing newline) tail.
        let l1 = r#"{"ts":"t1","from":"u","to":"agent-alpha","message":"a"}"#;
        let l2 = r#"{"ts":"t2","from":"u","to":"agent-alpha","message":"b"}"#;
        let partial = r#"{"ts":"t3","partial"#;
        fs::write(&path, format!("{l1}\n{l2}\n{partial}")).unwrap();
        let (consumed, lines) = read_complete_lines_from(&path, 0).unwrap();
        // Only the two newline-terminated lines are returned; the partial
        // is left for the next tick, so consumed stops before it.
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].as_ref().unwrap()["message"], "a");
        assert_eq!(lines[1].as_ref().unwrap()["message"], "b");
        let full = fs::metadata(&path).unwrap().len();
        assert!(
            consumed < full,
            "partial trailing line must not be consumed"
        );
        // A second read from the new offset finds nothing more yet.
        let (consumed2, lines2) = read_complete_lines_from(&path, consumed).unwrap();
        assert_eq!(consumed2, 0);
        assert!(lines2.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
