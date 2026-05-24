//! `bwoc team …` + `bwoc task …` — Saṅgha v1 CLI (Phase A foundation).
//!
//! Thin IO + locking layer over `bwoc_core::team`. The data model and
//! state-transition rules live in core; this module resolves the
//! workspace, reads/writes the on-disk files, and serializes task
//! mutations behind an advisory lock so two teammates never claim the
//! same task.
//!
//! Layout under the workspace:
//!
//! ```text
//! .bwoc/teams/<team-id>.toml          # Team membership
//! .bwoc/teams/<team-id>/tasks.jsonl   # shared task list
//! .bwoc/teams/<team-id>/tasks.lock    # advisory lock (this module)
//! ```
//!
//! The human operator is the implicit lead — there is no `lead` field.
//! Teammates **self-claim** tasks; the lock makes the claim a
//! Saṅghakamma (a communal act settled by exactly one member).

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use bwoc_core::manifest::Manifest;
use bwoc_core::team::{self, Task, Team, TeamError};
use bwoc_core::workspace::AgentsRegistry;

// --- path helpers ----------------------------------------------------------

fn teams_dir(workspace: &Path) -> PathBuf {
    workspace.join(".bwoc/teams")
}

fn team_toml_path(workspace: &Path, team_id: &str) -> PathBuf {
    teams_dir(workspace).join(format!("{team_id}.toml"))
}

fn team_task_dir(workspace: &Path, team_id: &str) -> PathBuf {
    teams_dir(workspace).join(team_id)
}

fn tasks_jsonl_path(workspace: &Path, team_id: &str) -> PathBuf {
    team_task_dir(workspace, team_id).join("tasks.jsonl")
}

/// Resolve the workspace per `WORKSPACE.en.md`: explicit → BWOC_WORKSPACE
/// → ancestor walk → None. (Local copy per the per-module convention used
/// by `chat`/`send`/`trust`.)
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

// --- advisory lock ---------------------------------------------------------

/// A best-effort advisory lock over a team's task file, acquired by
/// atomically creating `tasks.lock` (`O_CREAT | O_EXCL` via `create_new`).
/// Dependency-free; staleness is detected with a signal-0 probe on the
/// recorded PID (a crash mid-claim leaves a lock whose owner is dead, so
/// we steal it). Released on drop.
struct TaskLock {
    path: PathBuf,
}

impl TaskLock {
    fn acquire(task_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(task_dir)?;
        let path = task_dir.join("tasks.lock");
        // ~5s budget: 50 attempts × 100ms. Inbox-rare contention, so a
        // generous budget costs nothing in the common case.
        for _ in 0..50 {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue; // retry immediately after stealing
                    }
                    sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "could not acquire task lock (another `bwoc task` holding it? \
             remove .bwoc/teams/<id>/tasks.lock if stale)",
        ))
    }
}

impl Drop for TaskLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// A lock is stale if its recorded PID is no longer alive. An unreadable
/// or non-numeric lock body is treated as NOT stale (conservative — don't
/// steal a lock we can't reason about).
fn lock_is_stale(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    match raw.trim().parse::<u32>() {
        Ok(pid) => !crate::livecheck::signal_zero_alive(pid),
        Err(_) => false,
    }
}

// --- shared loaders --------------------------------------------------------

fn load_team(workspace: &Path, team_id: &str) -> Result<Team, String> {
    let path = team_toml_path(workspace, team_id);
    let body = fs::read_to_string(&path).map_err(|_| {
        format!(
            "no team '{team_id}' in workspace (expected {})",
            path.display()
        )
    })?;
    Team::from_toml(&body).map_err(|e| format!("team '{team_id}' is malformed: {e}"))
}

fn load_tasks(workspace: &Path, team_id: &str) -> Result<Vec<Task>, String> {
    let path = tasks_jsonl_path(workspace, team_id);
    match fs::read_to_string(&path) {
        Ok(body) => team::parse_tasks(&body).map_err(|e| format!("tasks.jsonl malformed: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("failed to read {}: {e}", path.display())),
    }
}

/// Atomic write: render to a sibling tmp file then rename over the target.
fn save_tasks(workspace: &Path, team_id: &str, tasks: &[Task]) -> Result<(), String> {
    let path = tasks_jsonl_path(workspace, team_id);
    let body = team::render_tasks(tasks).map_err(|e| format!("serialize failed: {e}"))?;
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, body).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .map_err(|e| format!("failed to rename into {}: {e}", path.display()))?;
    Ok(())
}

// --- team commands ---------------------------------------------------------

pub fn run_team_create(
    workspace: Option<PathBuf>,
    id: String,
    members: Vec<String>,
    json: bool,
) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc team create: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    let path = team_toml_path(&ws, &id);
    if path.exists() {
        eprintln!(
            "bwoc team create: team '{id}' already exists ({})",
            path.display()
        );
        return 2;
    }
    let team = Team::new(&id, members);
    let toml = match team.to_toml() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc team create: {e}");
            return 1;
        }
    };
    if let Err(e) = fs::create_dir_all(teams_dir(&ws)).and_then(|_| fs::write(&path, toml)) {
        eprintln!("bwoc team create: failed to write {}: {e}", path.display());
        return 1;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({
                "team": team.id,
                "members": team.members,
                "created_at": team.created_at,
                "path": path.display().to_string(),
            })
        );
    } else {
        println!(
            "Created team '{}' ({} member(s))",
            team.id,
            team.members.len()
        );
        for m in &team.members {
            println!("  - {m}");
        }
        println!("  Tasks: {}", tasks_jsonl_path(&ws, &id).display());
    }
    0
}

pub fn run_team_list(workspace: Option<PathBuf>, json: bool) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc team list: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    let dir = teams_dir(&ws);
    let mut teams: Vec<Team> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("toml")
                && let Ok(body) = fs::read_to_string(&p)
                && let Ok(t) = Team::from_toml(&body)
            {
                teams.push(t);
            }
        }
    }
    teams.sort_by(|a, b| a.id.cmp(&b.id));

    if json {
        let arr: Vec<_> = teams
            .iter()
            .map(|t| {
                serde_json::json!({
                    "team": t.id,
                    "members": t.members,
                    "created_at": t.created_at,
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(arr));
        return 0;
    }
    if teams.is_empty() {
        println!(
            "(no teams in workspace {} — `bwoc team create <id> --members …`)",
            ws.display()
        );
        return 0;
    }
    for t in &teams {
        let tasks = load_tasks(&ws, &t.id).unwrap_or_default();
        let pending = tasks
            .iter()
            .filter(|x| x.state == bwoc_core::team::TaskState::Pending)
            .count();
        println!(
            "{}  ({} member(s), {} task(s), {} pending)",
            t.id,
            t.members.len(),
            tasks.len(),
            pending
        );
    }
    0
}

pub fn run_team_retire(workspace: Option<PathBuf>, id: String, yes: bool, json: bool) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc team retire: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    let toml_path = team_toml_path(&ws, &id);
    if !toml_path.exists() {
        eprintln!("bwoc team retire: no team '{id}' in workspace");
        return 2;
    }
    if !yes {
        if !io::stdin().is_terminal() {
            eprintln!(
                "bwoc team retire: not a TTY and --yes not given. \
                 Pass --yes to confirm or run from an interactive shell."
            );
            return 2;
        }
        print!("Retire team '{id}' and delete its task list? [y/N]: ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return 1;
        }
        let a = line.trim().to_ascii_lowercase();
        if a != "y" && a != "yes" {
            eprintln!("aborted");
            return 2;
        }
    }
    let task_dir = team_task_dir(&ws, &id);
    let _ = fs::remove_dir_all(&task_dir); // best-effort (may not exist)
    if let Err(e) = fs::remove_file(&toml_path) {
        eprintln!(
            "bwoc team retire: failed to remove {}: {e}",
            toml_path.display()
        );
        return 1;
    }
    if json {
        println!("{}", serde_json::json!({ "team": id, "retired": true }));
    } else {
        println!("Retired team '{id}' (membership + task list removed)");
    }
    0
}

// --- task hooks ------------------------------------------------------------

/// Run a workspace-level task hook if one exists. Hooks live at
/// `<ws>/.bwoc/hooks/<event>` (`task-created` / `task-completed`); they are
/// optional — a missing or non-executable file is a silent no-op. The hook
/// receives the task context as environment variables (`BWOC_TASK_EVENT`,
/// `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_TASK_TITLE`, and `BWOC_AGENT` for
/// completion). A **non-zero exit blocks the operation** (matching Claude
/// Agent Teams' TaskCreated/TaskCompleted semantics): the caller aborts
/// before persisting, and the hook's stderr is surfaced. Returns
/// `Ok(())` when the hook passes or is absent; `Err(reason)` when it blocks
/// or can't be run.
fn run_task_hook(workspace: &Path, event: &str, env: &[(&str, &str)]) -> Result<(), String> {
    let hook = workspace.join(".bwoc/hooks").join(event);
    let meta = match std::fs::metadata(&hook) {
        Ok(m) => m,
        Err(_) => return Ok(()), // no hook installed → no-op
    };
    // Require the executable bit on Unix; on other platforms, presence is enough.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Ok(()); // present but not executable → treat as disabled
        }
    }
    #[cfg(not(unix))]
    let _ = &meta;

    let output = std::process::Command::new(&hook)
        .envs(env.iter().copied())
        .current_dir(workspace)
        .output()
        .map_err(|e| format!("hook {event} failed to run: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.trim().lines().next().unwrap_or("(no message)");
        Err(format!(
            "blocked by {event} hook (exit {}): {first}",
            output.status.code().unwrap_or(-1)
        ))
    }
}

// --- task commands ---------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn run_task_add(
    workspace: Option<PathBuf>,
    team_id: String,
    title: String,
    deps: Vec<String>,
    id_override: Option<String>,
    requires_plan: bool,
    json: bool,
) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc task add: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    if let Err(e) = load_team(&ws, &team_id) {
        eprintln!("bwoc task add: {e}");
        return 2;
    }
    let task_dir = team_task_dir(&ws, &team_id);
    let _lock = match TaskLock::acquire(&task_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bwoc task add: {e}");
            return 1;
        }
    };
    let mut tasks = match load_tasks(&ws, &team_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc task add: {e}");
            return 1;
        }
    };
    // Monotonic id `t<N>` (tasks are never removed, so len only grows),
    // unless the caller passed an explicit --id.
    let id = id_override.unwrap_or_else(|| format!("t{}", tasks.len() + 1));
    let mut task = Task::new(&id, &title, deps);
    task.requires_plan = requires_plan;
    if let Err(e) = team::add_task(&mut tasks, task) {
        eprintln!("bwoc task add: {e}");
        return 2;
    }
    // task-created hook (optional). A non-zero exit blocks creation — the
    // task is added in-memory but never persisted, so the file is unchanged.
    if let Err(e) = run_task_hook(
        &ws,
        "task-created",
        &[
            ("BWOC_TASK_EVENT", "task-created"),
            ("BWOC_TEAM", &team_id),
            ("BWOC_TASK_ID", &id),
            ("BWOC_TASK_TITLE", &title),
        ],
    ) {
        eprintln!("bwoc task add: {e}");
        return 2;
    }
    if let Err(e) = save_tasks(&ws, &team_id, &tasks) {
        eprintln!("bwoc task add: {e}");
        return 1;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({ "team": team_id, "task": id, "state": "pending" })
        );
    } else {
        println!("Added task '{id}' to team '{team_id}': {title}");
    }
    0
}

pub fn run_task_list(workspace: Option<PathBuf>, team_id: String, json: bool) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc task list: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    if let Err(e) = load_team(&ws, &team_id) {
        eprintln!("bwoc task list: {e}");
        return 2;
    }
    let tasks = match load_tasks(&ws, &team_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc task list: {e}");
            return 1;
        }
    };
    if json {
        let body = team::render_tasks(&tasks).unwrap_or_default();
        // Emit a JSON array (parse each line back) for consumer-friendliness.
        let arr: Vec<serde_json::Value> = body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        println!("{}", serde_json::Value::Array(arr));
        return 0;
    }
    if tasks.is_empty() {
        println!("(team '{team_id}' has no tasks — `bwoc task add {team_id} \"…\"`)");
        return 0;
    }
    for t in &tasks {
        let owner = t.claimed_by.as_deref().unwrap_or("—");
        let deps = if t.deps.is_empty() {
            String::new()
        } else {
            format!("  deps=[{}]", t.deps.join(","))
        };
        println!(
            "{:<6} {:<12} {:<14} {}{}",
            t.id,
            t.state.as_str(),
            owner,
            t.title,
            deps
        );
    }
    0
}

pub fn run_task_claim(
    workspace: Option<PathBuf>,
    team_id: String,
    task_id: String,
    agent: String,
    json: bool,
) -> i32 {
    mutate_task(
        workspace,
        &team_id,
        &task_id,
        &agent,
        json,
        "claim",
        |tasks| team::claim_task(tasks, &task_id, &agent),
    )
}

pub fn run_task_complete(
    workspace: Option<PathBuf>,
    team_id: String,
    task_id: String,
    agent: String,
    json: bool,
) -> i32 {
    mutate_task(
        workspace,
        &team_id,
        &task_id,
        &agent,
        json,
        "complete",
        |tasks| team::complete_task(tasks, &task_id, &agent),
    )
}

/// Shared claim/complete path: resolve workspace, verify the actor is a
/// team member, acquire the lock, load → mutate → save. The `op` closure
/// runs the core transition; `verb` is just for messages.
fn mutate_task(
    workspace: Option<PathBuf>,
    team_id: &str,
    task_id: &str,
    agent: &str,
    json: bool,
    verb: &str,
    op: impl FnOnce(&mut [Task]) -> Result<(), TeamError>,
) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc task {verb}: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    let team = match load_team(&ws, team_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc task {verb}: {e}");
            return 2;
        }
    };
    if let Err(e) = team::ensure_member(&team, agent) {
        eprintln!(
            "bwoc task {verb}: {e} (members: {})",
            team.members.join(", ")
        );
        return 2;
    }
    let task_dir = team_task_dir(&ws, team_id);
    let _lock = match TaskLock::acquire(&task_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bwoc task {verb}: {e}");
            return 1;
        }
    };
    let mut tasks = match load_tasks(&ws, team_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc task {verb}: {e}");
            return 1;
        }
    };
    if let Err(e) = op(&mut tasks) {
        eprintln!("bwoc task {verb}: {e}");
        // Transition errors (wrong state, blocked, not claimant) are user
        // errors → exit 2; the file is untouched.
        return 2;
    }
    // Lifecycle hooks (optional). The transition succeeded in-memory;
    // a non-zero exit blocks the operation by aborting before the file is
    // written. Two events:
    //   task-claimed   — fires when verb == "claim" (Track B, Phase 3).
    //   task-completed — fires when verb == "complete".
    if verb == "claim" {
        // Resolve the claimant's worktreeBase from their config.manifest.json
        // so the hook can construct the exact worktree path without parsing
        // any agent-written log.  Fall back to "/tmp" if the manifest is
        // absent or the field is unset (non-fatal — the hook still runs).
        let worktree_base = AgentsRegistry::load(&ws)
            .ok()
            .and_then(|reg| {
                reg.agents
                    .into_iter()
                    .find(|e| e.id == agent || e.id == format!("agent-{agent}"))
            })
            .and_then(|entry| {
                Manifest::load_from_path(&ws.join(&entry.path).join("config.manifest.json")).ok()
            })
            .and_then(|m| m.worktree_base)
            .unwrap_or_else(|| "/tmp".to_owned());
        if let Err(e) = run_task_hook(
            &ws,
            "task-claimed",
            &[
                ("BWOC_TASK_EVENT", "task-claimed"),
                ("BWOC_TEAM", team_id),
                ("BWOC_TASK_ID", task_id),
                ("BWOC_AGENT", agent),
                ("BWOC_WORKTREE_BASE", &worktree_base),
            ],
        ) {
            eprintln!("bwoc task {verb}: {e}");
            return 2;
        }
    }
    if verb == "complete"
        && let Err(e) = run_task_hook(
            &ws,
            "task-completed",
            &[
                ("BWOC_TASK_EVENT", "task-completed"),
                ("BWOC_TEAM", team_id),
                ("BWOC_TASK_ID", task_id),
                ("BWOC_AGENT", agent),
            ],
        )
    {
        eprintln!("bwoc task {verb}: {e}");
        return 2;
    }
    if let Err(e) = save_tasks(&ws, team_id, &tasks) {
        eprintln!("bwoc task {verb}: {e}");
        return 1;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({ "team": team_id, "agent": agent, "op": verb, "ok": true })
        );
    } else {
        println!("{verb}: ok ({agent} on team '{team_id}')");
    }
    0
}

// --- plan approval (Pavāraṇā) ----------------------------------------------

/// `bwoc task plan <team> <task> [--as <agent>] [--plan … | --plan-file …]`.
/// With plan content → submit/revise (requires `--as`, member-guarded,
/// locked). Without → show the current plan + verdict (read-only).
pub fn run_task_plan(
    workspace: Option<PathBuf>,
    team_id: String,
    task_id: String,
    agent: Option<String>,
    plan: Option<String>,
    json: bool,
) -> i32 {
    // Submit path: plan content present.
    if let Some(plan_text) = plan {
        let Some(agent) = agent else {
            eprintln!("bwoc task plan: --as <agent> is required to submit a plan");
            return 2;
        };
        return mutate_task(
            workspace,
            &team_id,
            &task_id,
            &agent,
            json,
            "plan",
            |tasks| team::submit_plan(tasks, &task_id, &agent, &plan_text),
        );
    }
    // Show path: no plan content → print the current plan + verdict.
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc task plan: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    let tasks = match load_tasks(&ws, &team_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc task plan: {e}");
            return 1;
        }
    };
    let Some(task) = tasks.iter().find(|t| t.id == task_id) else {
        eprintln!("bwoc task plan: task '{task_id}' not found in team '{team_id}'");
        return 2;
    };
    let verdict = match task.plan_approved {
        None if task.plan.is_some() => "pending review",
        None => "not submitted",
        Some(true) => "approved",
        Some(false) => "rejected",
    };
    if json {
        println!(
            "{}",
            serde_json::json!({
                "team": team_id,
                "task": task_id,
                "requires_plan": task.requires_plan,
                "plan": task.plan,
                "verdict": verdict,
            })
        );
    } else {
        println!("Task {team_id}/{task_id} — plan: {verdict}");
        match &task.plan {
            Some(p) => {
                println!("---");
                println!("{p}");
                println!("---");
            }
            None => println!("(no plan submitted yet)"),
        }
    }
    0
}

/// `bwoc task approve|reject <team> <task>` — the lead's Pavāraṇā verdict.
/// No `--as`: the human operator is the implicit lead. Locked + saved.
pub fn run_task_review(
    workspace: Option<PathBuf>,
    team_id: String,
    task_id: String,
    approved: bool,
    json: bool,
) -> i32 {
    let verb = if approved { "approve" } else { "reject" };
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!("bwoc task {verb}: no workspace found. Pass --workspace or run `bwoc init`.");
        return 2;
    };
    if let Err(e) = load_team(&ws, &team_id) {
        eprintln!("bwoc task {verb}: {e}");
        return 2;
    }
    let task_dir = team_task_dir(&ws, &team_id);
    let _lock = match TaskLock::acquire(&task_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bwoc task {verb}: {e}");
            return 1;
        }
    };
    let mut tasks = match load_tasks(&ws, &team_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc task {verb}: {e}");
            return 1;
        }
    };
    if let Err(e) = team::review_plan(&mut tasks, &task_id, approved) {
        eprintln!("bwoc task {verb}: {e}");
        return 2;
    }
    if let Err(e) = save_tasks(&ws, &team_id, &tasks) {
        eprintln!("bwoc task {verb}: {e}");
        return 1;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({ "team": team_id, "task": task_id, "approved": approved })
        );
    } else if approved {
        println!("Approved plan for {team_id}/{task_id} — claimant may now complete it");
    } else {
        println!("Rejected plan for {team_id}/{task_id} — claimant must revise + resubmit");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn task_hook_missing_is_noop_blocking_is_err() {
        use std::os::unix::fs::PermissionsExt;
        // Unique, auto-cleaned dir per test — never a PID-keyed shared /tmp path
        // (a leftover dir from a recycled PID could carry a stale hook and flake).
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_path_buf();
        let hooks = ws.join(".bwoc/hooks");
        std::fs::create_dir_all(&hooks).unwrap();

        // No hook installed → Ok (no-op).
        assert!(run_task_hook(&ws, "task-created", &[]).is_ok());

        // Non-executable hook → treated as disabled → Ok.
        let hook = hooks.join("task-created");
        std::fs::write(&hook, "#!/bin/sh\nexit 2\n").unwrap();
        assert!(run_task_hook(&ws, "task-created", &[]).is_ok());

        // Executable + exit 0 → Ok.
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(run_task_hook(&ws, "task-created", &[]).is_ok());

        // Executable + exit 2 → Err (blocks), surfaces stderr.
        std::fs::write(&hook, "#!/bin/sh\necho nope >&2\nexit 2\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = run_task_hook(&ws, "task-created", &[]).unwrap_err();
        assert!(err.contains("blocked by task-created hook"), "got: {err}");
        assert!(err.contains("nope"), "stderr surfaced: {err}");
        // `tmp` drops here → dir auto-removed.
    }

    /// task-claimed hook fires with the right env vars and blocks on non-zero exit.
    /// Mirrors the task-created / task-completed test pattern.
    #[cfg(unix)]
    #[test]
    fn task_claimed_hook_receives_env_and_blocks() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_path_buf();
        let hooks = ws.join(".bwoc/hooks");
        std::fs::create_dir_all(&hooks).unwrap();

        // Missing hook → no-op (same as other hook events).
        assert!(run_task_hook(&ws, "task-claimed", &[]).is_ok());

        // Executable hook that exits 0 and echoes the env vars to a temp file
        // so we can verify they were received.
        let log = ws.join("claimed.log");
        let log_str = log.to_string_lossy().to_string();
        let hook = hooks.join("task-claimed");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\necho \"$BWOC_TASK_EVENT $BWOC_TEAM $BWOC_TASK_ID $BWOC_AGENT $BWOC_WORKTREE_BASE\" > {log_str}\nexit 0\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

        let result = run_task_hook(
            &ws,
            "task-claimed",
            &[
                ("BWOC_TASK_EVENT", "task-claimed"),
                ("BWOC_TEAM", "squad"),
                ("BWOC_TASK_ID", "t1"),
                ("BWOC_AGENT", "agent-pi"),
                ("BWOC_WORKTREE_BASE", "/tmp"),
            ],
        );
        assert!(result.is_ok(), "hook exit 0 must not block: {result:?}");
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            logged.contains("task-claimed"),
            "BWOC_TASK_EVENT missing: {logged}"
        );
        assert!(logged.contains("squad"), "BWOC_TEAM missing: {logged}");
        assert!(logged.contains("t1"), "BWOC_TASK_ID missing: {logged}");
        assert!(logged.contains("agent-pi"), "BWOC_AGENT missing: {logged}");
        assert!(
            logged.contains("/tmp"),
            "BWOC_WORKTREE_BASE missing: {logged}"
        );

        // Non-zero exit → blocks the claim.
        std::fs::write(&hook, "#!/bin/sh\necho 'no new claims' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err =
            run_task_hook(&ws, "task-claimed", &[("BWOC_TASK_EVENT", "task-claimed")]).unwrap_err();
        assert!(err.contains("blocked by task-claimed hook"), "got: {err}");
        assert!(err.contains("no new claims"), "stderr surfaced: {err}");
        // `tmp` drops here → dir auto-removed.
    }
}
