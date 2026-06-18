//! `bwoc tasks` — a fleet-wide, queryable view over every team's task list.
//!
//! The per-team `bwoc task list <team>` answers "what's on *this* team's list".
//! This is the cross-team aggregate (#300): it scans every
//! `.bwoc/teams/<id>/tasks.jsonl`, filters by `--agent` (claimant) and `--state`
//! (`pending` / `in_progress` / `completed`), and prints a table or `--json`.
//! Task state already lives explicitly in the JSONL (not just in a file's
//! location), so this is a read-only query over the existing store.

use std::path::{Path, PathBuf};

use bwoc_core::team::{self, Task, TaskState};

pub struct TasksArgs {
    pub workspace: Option<PathBuf>,
    /// Filter to tasks claimed by this agent (id or bare name).
    pub agent: Option<String>,
    /// Filter to one state: `pending` | `in_progress` | `completed`.
    pub state: Option<String>,
    pub json: bool,
}

pub fn run(args: TasksArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc tasks: no workspace (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`."
        );
        return 2;
    };

    let state_filter = match args.state.as_deref() {
        None => None,
        Some(s) => match parse_state(s) {
            Some(st) => Some(st),
            None => {
                eprintln!("bwoc tasks: unknown --state `{s}` (pending | in_progress | completed)");
                return 2;
            }
        },
    };
    let agent_filter = args.agent.as_deref().map(normalize_agent);

    let mut rows: Vec<(String, Task)> = Vec::new();
    let teams_dir = workspace.join(".bwoc").join("teams");
    if let Ok(entries) = std::fs::read_dir(&teams_dir) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let team_id = entry.file_name().to_string_lossy().into_owned();
            let tasks_path = dir.join("tasks.jsonl");
            let Ok(raw) = std::fs::read_to_string(&tasks_path) else {
                continue; // a team dir with no task list yet — skip silently
            };
            let tasks = match team::parse_tasks(&raw) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("bwoc tasks: skipping {} ({e})", tasks_path.display());
                    continue;
                }
            };
            for t in tasks {
                if let Some(st) = state_filter {
                    if t.state != st {
                        continue;
                    }
                }
                if let Some(ag) = &agent_filter {
                    if t.claimed_by.as_deref() != Some(ag.as_str()) {
                        continue;
                    }
                }
                rows.push((team_id.clone(), t));
            }
        }
    }
    // Stable order: team, then state, then id.
    rows.sort_by(|a, b| {
        (a.0.as_str(), a.1.state.as_str(), a.1.id.as_str()).cmp(&(
            b.0.as_str(),
            b.1.state.as_str(),
            b.1.id.as_str(),
        ))
    });

    if args.json {
        emit_json(&workspace, &rows);
    } else {
        print_table(&workspace, &rows);
    }
    0
}

fn parse_state(s: &str) -> Option<TaskState> {
    match s {
        "pending" => Some(TaskState::Pending),
        "in_progress" | "in-progress" => Some(TaskState::InProgress),
        "completed" | "done" => Some(TaskState::Completed),
        _ => None,
    }
}

fn normalize_agent(a: &str) -> String {
    if a.starts_with("agent-") {
        a.to_string()
    } else {
        format!("agent-{a}")
    }
}

fn print_table(workspace: &Path, rows: &[(String, Task)]) {
    println!();
    println!("Tasks — {} ({} task(s))", workspace.display(), rows.len());
    println!();
    if rows.is_empty() {
        println!("(no tasks match — try without filters, or `bwoc team list`)");
        println!();
        return;
    }
    let team_w = rows.iter().map(|(t, _)| t.len()).max().unwrap_or(4).max(4);
    let id_w = rows
        .iter()
        .map(|(_, t)| t.id.len())
        .max()
        .unwrap_or(2)
        .max(2);
    let claim_w = rows
        .iter()
        .map(|(_, t)| t.claimed_by.as_deref().unwrap_or("—").len())
        .max()
        .unwrap_or(9)
        .max(9);
    println!(
        "  {:<team_w$}  {:<id_w$}  {:<12}  {:<claim_w$}  TITLE",
        "TEAM", "ID", "STATE", "CLAIMED-BY"
    );
    for (team, t) in rows {
        println!(
            "  {:<team_w$}  {:<id_w$}  {:<12}  {:<claim_w$}  {}",
            team,
            t.id,
            t.state.as_str(),
            t.claimed_by.as_deref().unwrap_or("—"),
            t.title,
        );
    }
    println!();
}

fn emit_json(workspace: &Path, rows: &[(String, Task)]) {
    let tasks: Vec<serde_json::Value> = rows
        .iter()
        .map(|(team, t)| {
            serde_json::json!({
                "team": team,
                "id": t.id,
                "state": t.state.as_str(),
                "claimed_by": t.claimed_by,
                "title": t.title,
                "created_at": t.created_at,
                "completed_at": t.completed_at,
            })
        })
        .collect();
    let value = serde_json::json!({
        "workspace": workspace.display().to_string(),
        "tasks": tasks,
    });
    match serde_json::to_string_pretty(&value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("bwoc tasks: failed to serialize: {e}"),
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
    use std::fs;

    fn setup(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-tasks-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        fs::write(
            root.join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"t\"\nversion=\"0.1.0\"\ncreated=\"2026-01-01T00:00:00Z\"\n",
        )
        .unwrap();
        root
    }

    fn write_team_tasks(root: &Path, team: &str, lines: &[String]) {
        let dir = root.join(".bwoc/teams").join(team);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tasks.jsonl"), format!("{}\n", lines.join("\n"))).unwrap();
    }

    fn task(id: &str, state: &str, claimed: Option<&str>) -> String {
        let mut v = serde_json::json!({
            "id": id, "title": format!("do {id}"), "state": state,
            "created_at": "2026-01-01T00:00:00Z",
        });
        if let Some(c) = claimed {
            v["claimed_by"] = serde_json::Value::String(c.to_string());
        }
        v.to_string()
    }

    #[test]
    fn parse_state_accepts_aliases_and_rejects_unknown() {
        assert_eq!(parse_state("in-progress"), Some(TaskState::InProgress));
        assert_eq!(parse_state("done"), Some(TaskState::Completed));
        assert_eq!(parse_state("nope"), None);
    }

    #[test]
    fn aggregates_across_teams_and_exits_zero() {
        let root = setup("agg");
        write_team_tasks(&root, "alpha", &[task("a1", "pending", None)]);
        write_team_tasks(
            &root,
            "beta",
            &[
                task("b1", "in_progress", Some("agent-pi")),
                task("b2", "completed", Some("agent-pi")),
            ],
        );
        // No filter → exit 0 (smoke that aggregation + render don't panic).
        assert_eq!(
            run(TasksArgs {
                workspace: Some(root.clone()),
                agent: None,
                state: None,
                json: true,
            }),
            0
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_state_filter_is_exit_2() {
        let root = setup("badstate");
        assert_eq!(
            run(TasksArgs {
                workspace: Some(root.clone()),
                agent: None,
                state: Some("frobnicated".into()),
                json: false,
            }),
            2
        );
        let _ = fs::remove_dir_all(&root);
    }
}
