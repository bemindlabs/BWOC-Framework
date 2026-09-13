//! Saṅgha — team membership + shared task list (Phase A foundation).
//!
//! A **team** groups a subset of a workspace's agents under a shared
//! task list. The human operator is the implicit lead (no `lead` field);
//! teammates **self-claim** pending, unblocked tasks. Task claiming is
//! made race-safe by a file lock held at the CLI layer — this module is
//! pure: it owns the data model and the state-transition rules, not the
//! IO locking.
//!
//! On-disk layout (under a workspace's `.bwoc/`):
//!
//! ```text
//! .bwoc/teams/<team-id>.toml          # membership (this module: Team)
//! .bwoc/teams/<team-id>/tasks.jsonl   # one Task per line (this module: Task)
//! .bwoc/teams/<team-id>/tasks.lock    # advisory lock (CLI layer only)
//! ```
//!
//! Maps to **Saṅgha** (the community of practitioners) and the claim
//! protocol to **Saṅghakamma** (a formal communal act: a task is claimed
//! by exactly one member, settled under a lock so two members never
//! claim the same item). Task dependencies encode the order in which
//! communal work must settle.

use serde::{Deserialize, Serialize};

use crate::time::utc_now_iso8601;

/// A team: a named subset of workspace agents sharing one task list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Team {
    /// Stable team id (kebab-case by convention; not enforced here).
    pub id: String,
    /// Agent ids that belong to the team. The human operator is the
    /// implicit lead and is never listed here.
    pub members: Vec<String>,
    /// Designated peer-review agent (HV3-3c). When set, the lead routes a
    /// worker's diff to this agent before completing the task; a rejection
    /// re-queues the task. `None` = no review gate (workers complete directly).
    /// By convention a team member, and never reviews its own work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    /// UTC ISO 8601 creation stamp.
    pub created_at: String,
}

impl Team {
    pub fn new(id: impl Into<String>, members: Vec<String>) -> Self {
        Self {
            id: id.into(),
            members,
            reviewer: None,
            created_at: utc_now_iso8601(),
        }
    }

    /// Parse a `<team-id>.toml` document.
    pub fn from_toml(s: &str) -> Result<Self, TeamError> {
        toml::from_str(s).map_err(|e| TeamError::Parse(e.to_string()))
    }

    /// Serialize to a `<team-id>.toml` document.
    pub fn to_toml(&self) -> Result<String, TeamError> {
        toml::to_string_pretty(self).map_err(|e| TeamError::Serialize(e.to_string()))
    }

    pub fn has_member(&self, agent_id: &str) -> bool {
        self.members.iter().any(|m| m == agent_id)
    }
}

/// Lifecycle state of a task. The three-state machine mirrors Claude
/// Agent Teams: `Pending → InProgress → Completed`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    InProgress,
    Completed,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Pending => "pending",
            TaskState::InProgress => "in_progress",
            TaskState::Completed => "completed",
        }
    }
}

/// One work item on a team's shared task list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Stable task id, unique within the team's task list.
    pub id: String,
    /// Human-readable summary of the work.
    pub title: String,
    pub state: TaskState,
    /// Task ids that must be `Completed` before this task can be claimed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
    /// Agent id that claimed the task (set on claim, kept through complete).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    /// UTC ISO 8601 creation stamp.
    pub created_at: String,
    /// UTC ISO 8601 completion stamp (set on complete).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Pavāraṇā gate: when true, the task cannot be completed until its
    /// submitted plan has been approved by the lead. Default false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub requires_plan: bool,
    /// The plan the claimant submitted for lead review (Pavāraṇā — the
    /// teammate invites judgment before proceeding). `None` until submitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Lead's verdict on the submitted plan: `None` = pending review,
    /// `Some(true)` = approved, `Some(false)` = rejected (resubmit needed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_approved: Option<bool>,
}

/// serde `skip_serializing_if` helper — keeps `requires_plan: false` out
/// of the JSONL so existing/simple tasks stay one-line and unchanged.
fn is_false(b: &bool) -> bool {
    !*b
}

impl Task {
    pub fn new(id: impl Into<String>, title: impl Into<String>, deps: Vec<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            state: TaskState::Pending,
            deps,
            claimed_by: None,
            created_at: utc_now_iso8601(),
            completed_at: None,
            requires_plan: false,
            plan: None,
            plan_approved: None,
        }
    }
}

/// Errors from team / task operations. The CLI layer maps these to exit
/// codes + actionable messages.
#[derive(Debug, thiserror::Error)]
pub enum TeamError {
    #[error("team toml parse error: {0}")]
    Parse(String),
    #[error("team toml serialize error: {0}")]
    Serialize(String),
    #[error("task '{0}' not found in this team")]
    TaskNotFound(String),
    #[error("task '{0}' already exists in this team")]
    DuplicateTask(String),
    #[error("task '{task}' depends on unknown task '{dep}'")]
    UnknownDependency { task: String, dep: String },
    #[error("task '{id}' is {state} — only pending tasks can be claimed")]
    NotClaimable { id: String, state: &'static str },
    #[error("task '{id}' is blocked: dependency '{dep}' is not completed")]
    BlockedByDependency { id: String, dep: String },
    #[error("task '{id}' is {state} — only in-progress tasks can be completed")]
    NotCompletable { id: String, state: &'static str },
    #[error("task '{id}' is claimed by '{owner}', not '{actor}'")]
    NotClaimant {
        id: String,
        owner: String,
        actor: String,
    },
    #[error("agent '{0}' is not a member of this team")]
    NotAMember(String),
    #[error("task '{id}' is {state} — a plan can only be submitted for a task you've claimed")]
    NotClaimedForPlan { id: String, state: &'static str },
    #[error(
        "task '{id}' has no submitted plan to review — the claimant must `bwoc task plan` first"
    )]
    NoPlanSubmitted { id: String },
    #[error("task '{id}' requires plan approval before completion (plan {plan_state})")]
    PlanNotApproved {
        id: String,
        plan_state: &'static str,
    },
    #[error("task '{id}' is {state} — only a completed task can be reopened")]
    NotReopenable { id: String, state: &'static str },
}

/// Append a new task. Rejects a duplicate id and any dependency that
/// doesn't resolve to an existing task in the list.
pub fn add_task(tasks: &mut Vec<Task>, task: Task) -> Result<(), TeamError> {
    if tasks.iter().any(|t| t.id == task.id) {
        return Err(TeamError::DuplicateTask(task.id));
    }
    for dep in &task.deps {
        if !tasks.iter().any(|t| &t.id == dep) {
            return Err(TeamError::UnknownDependency {
                task: task.id.clone(),
                dep: dep.clone(),
            });
        }
    }
    tasks.push(task);
    Ok(())
}

/// Claim a pending, unblocked task for `agent`. A task is claimable iff
/// it is `Pending` and every dependency is `Completed`. Self-claim is
/// the model (any member may claim) — membership is enforced by the
/// caller via [`ensure_member`] before this is reached.
pub fn claim_task(tasks: &mut [Task], id: &str, agent: &str) -> Result<(), TeamError> {
    // Resolve dependency completion against a snapshot first (avoids a
    // double mutable borrow when we then locate the target task).
    let dep_states: Vec<(String, TaskState)> =
        tasks.iter().map(|t| (t.id.clone(), t.state)).collect();

    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| TeamError::TaskNotFound(id.to_string()))?;

    if task.state != TaskState::Pending {
        return Err(TeamError::NotClaimable {
            id: id.to_string(),
            state: task.state.as_str(),
        });
    }
    for dep in &task.deps {
        let completed = dep_states
            .iter()
            .any(|(did, st)| did == dep && *st == TaskState::Completed);
        if !completed {
            return Err(TeamError::BlockedByDependency {
                id: id.to_string(),
                dep: dep.clone(),
            });
        }
    }
    task.state = TaskState::InProgress;
    task.claimed_by = Some(agent.to_string());
    Ok(())
}

/// Complete an in-progress task. Only the claiming agent may complete it.
pub fn complete_task(tasks: &mut [Task], id: &str, agent: &str) -> Result<(), TeamError> {
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| TeamError::TaskNotFound(id.to_string()))?;

    if task.state != TaskState::InProgress {
        return Err(TeamError::NotCompletable {
            id: id.to_string(),
            state: task.state.as_str(),
        });
    }
    match &task.claimed_by {
        Some(owner) if owner == agent => {}
        Some(owner) => {
            return Err(TeamError::NotClaimant {
                id: id.to_string(),
                owner: owner.clone(),
                actor: agent.to_string(),
            });
        }
        None => {
            return Err(TeamError::NotClaimant {
                id: id.to_string(),
                owner: "<unclaimed>".to_string(),
                actor: agent.to_string(),
            });
        }
    }
    // Pavāraṇā gate: a plan-required task cannot complete until the lead
    // has approved the submitted plan.
    if task.requires_plan && task.plan_approved != Some(true) {
        let plan_state = match (task.plan.is_some(), task.plan_approved) {
            (false, _) => "not submitted",
            (true, None) => "pending review",
            (true, Some(false)) => "rejected",
            (true, Some(true)) => unreachable!(),
        };
        return Err(TeamError::PlanNotApproved {
            id: id.to_string(),
            plan_state,
        });
    }
    task.state = TaskState::Completed;
    task.completed_at = Some(utc_now_iso8601());
    Ok(())
}

/// Reopen a completed task, returning it to `Pending`.
///
/// # Why this exists
///
/// A completion is an assertion about the world, and an agent can assert
/// something false. The first `--loop` run against a real task list marked
/// "merge branch X to main" completed by a worker that had merged nothing —
/// and because the state machine ran one way only, the shared list carried
/// that falsehood with no way to withdraw it. Every downstream task the
/// completion unblocked inherited it.
///
/// So this is not an undo convenience. It is the correction path a shared
/// record needs in order to stay trustworthy: without one, the only fix is
/// hand-editing `tasks.jsonl` outside the locked `bwoc task` path, which
/// leaves no trace of what was corrected or why.
///
/// # What it clears, and what it keeps
///
/// Reopening drops `claimed_by`, `completed_at`, and — for a plan-gated task
/// — the approval verdict, because all three describe work that is no longer
/// asserted to have happened. A stale `Some(true)` verdict would let the next
/// claimant complete the task without a fresh plan, which is exactly the gate
/// this incident proved is load-bearing.
///
/// The **plan text itself is kept**: it is the claimant's own account of what
/// they intended, useful to whoever picks the task up, and destroying it would
/// discard evidence rather than correct a claim.
///
/// # Dependents
///
/// Tasks that depend on this one are left alone deliberately. `claim_task`
/// re-checks dependency state at claim time, so a dependent that has not yet
/// been claimed becomes unclaimable again on its own. One that is already
/// `InProgress` or `Completed` is a judgment call for the operator — silently
/// cascading a reopen through the graph would withdraw assertions nobody asked
/// this call to touch. [`dependents_of`] surfaces them so the caller can
/// report them instead of guessing.
pub fn reopen_task(tasks: &mut [Task], id: &str) -> Result<(), TeamError> {
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| TeamError::TaskNotFound(id.to_string()))?;

    if task.state != TaskState::Completed {
        return Err(TeamError::NotReopenable {
            id: id.to_string(),
            state: task.state.as_str(),
        });
    }

    task.state = TaskState::Pending;
    task.claimed_by = None;
    task.completed_at = None;
    // A plan-gated task must be re-approved: a surviving verdict would let the
    // next claimant complete without one.
    if task.requires_plan {
        task.plan_approved = None;
    }
    Ok(())
}

/// Ids of the tasks that list `id` as a dependency, in list order.
///
/// For reporting after a [`reopen_task`] — the caller can name what the
/// reopened task was holding up rather than leaving the operator to grep.
pub fn dependents_of(tasks: &[Task], id: &str) -> Vec<String> {
    tasks
        .iter()
        .filter(|t| t.deps.iter().any(|d| d == id))
        .map(|t| t.id.clone())
        .collect()
}

/// Submit (or revise) a plan for a task the agent has claimed — Pavāraṇā,
/// inviting the lead's judgment before proceeding. The task must be
/// `InProgress` and claimed by `agent`. Resets any prior verdict to
/// pending (a resubmission after rejection awaits fresh review).
pub fn submit_plan(tasks: &mut [Task], id: &str, agent: &str, plan: &str) -> Result<(), TeamError> {
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| TeamError::TaskNotFound(id.to_string()))?;
    if task.state != TaskState::InProgress {
        return Err(TeamError::NotClaimedForPlan {
            id: id.to_string(),
            state: task.state.as_str(),
        });
    }
    match &task.claimed_by {
        Some(owner) if owner == agent => {}
        owner => {
            return Err(TeamError::NotClaimant {
                id: id.to_string(),
                owner: owner.clone().unwrap_or_else(|| "<unclaimed>".to_string()),
                actor: agent.to_string(),
            });
        }
    }
    task.plan = Some(plan.to_string());
    task.plan_approved = None; // fresh submission awaits review
    Ok(())
}

/// Lead verdict on a submitted plan. `approved` true = approve, false =
/// reject (the claimant must revise + resubmit). The task must have a
/// plan on file. The lead is the human operator — no agent identity here.
pub fn review_plan(tasks: &mut [Task], id: &str, approved: bool) -> Result<(), TeamError> {
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| TeamError::TaskNotFound(id.to_string()))?;
    if task.plan.is_none() {
        return Err(TeamError::NoPlanSubmitted { id: id.to_string() });
    }
    task.plan_approved = Some(approved);
    Ok(())
}

/// Parse a `tasks.jsonl` body into a task vector. Blank lines are
/// skipped; a malformed line is an error (the file is machine-written,
/// so corruption is worth surfacing rather than silently dropping work).
pub fn parse_tasks(jsonl: &str) -> Result<Vec<Task>, TeamError> {
    let mut out = Vec::new();
    for line in jsonl.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let task: Task =
            serde_json::from_str(trimmed).map_err(|e| TeamError::Parse(e.to_string()))?;
        out.push(task);
    }
    Ok(out)
}

/// Serialize a task vector to a `tasks.jsonl` body (one task per line).
pub fn render_tasks(tasks: &[Task]) -> Result<String, TeamError> {
    let mut out = String::new();
    for t in tasks {
        let line = serde_json::to_string(t).map_err(|e| TeamError::Serialize(e.to_string()))?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Team chat (HV3-3a) — the shared broadcast channel
// ---------------------------------------------------------------------------

/// One message on a team's shared chat log.
///
/// Append-only, one JSON object per line in
/// `.bwoc/teams/<team-id>/chat.jsonl` — the same storage model as the task
/// list, so members in a team see each other's replies through the shared
/// filesystem with no pub/sub infrastructure. As with `tasks.jsonl`, the file
/// lock that makes concurrent appends race-safe is the CLI/host layer's
/// concern; this module owns only the data model.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TeamChatMessage {
    /// Agent id that sent the message (the human lead may use a reserved id).
    pub from: String,
    /// The message body.
    pub text: String,
    /// UTC ISO 8601 send stamp.
    pub ts: String,
}

impl TeamChatMessage {
    pub fn new(from: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            text: text.into(),
            ts: utc_now_iso8601(),
        }
    }

    /// Serialize as one `chat.jsonl` line (no trailing newline — the caller
    /// appends `\n`). Lets a host append a single message without rewriting
    /// the whole log.
    pub fn to_line(&self) -> Result<String, TeamError> {
        serde_json::to_string(self).map_err(|e| TeamError::Serialize(e.to_string()))
    }
}

/// Parse a `chat.jsonl` body into a message vector. Blank lines are skipped;
/// a malformed line is an error (machine-written, like the task list).
pub fn parse_chat(jsonl: &str) -> Result<Vec<TeamChatMessage>, TeamError> {
    let mut out = Vec::new();
    for line in jsonl.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: TeamChatMessage =
            serde_json::from_str(trimmed).map_err(|e| TeamError::Parse(e.to_string()))?;
        out.push(msg);
    }
    Ok(out)
}

/// Serialize a message vector to a `chat.jsonl` body (one per line).
pub fn render_chat(msgs: &[TeamChatMessage]) -> Result<String, TeamError> {
    let mut out = String::new();
    for m in msgs {
        out.push_str(&m.to_line()?);
        out.push('\n');
    }
    Ok(out)
}

/// Guard: the actor must be a team member to act on its tasks.
pub fn ensure_member(team: &Team, agent: &str) -> Result<(), TeamError> {
    if team.has_member(agent) {
        Ok(())
    } else {
        Err(TeamError::NotAMember(agent.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Vec<Task> {
        vec![Task::new("t1", "first", vec![])]
    }

    #[test]
    fn add_rejects_duplicate() {
        let mut tasks = seed();
        let err = add_task(&mut tasks, Task::new("t1", "dup", vec![])).unwrap_err();
        assert!(matches!(err, TeamError::DuplicateTask(_)));
    }

    #[test]
    fn add_rejects_unknown_dependency() {
        let mut tasks = seed();
        let err =
            add_task(&mut tasks, Task::new("t2", "second", vec!["ghost".into()])).unwrap_err();
        assert!(matches!(err, TeamError::UnknownDependency { .. }));
    }

    #[test]
    fn add_accepts_known_dependency() {
        let mut tasks = seed();
        add_task(&mut tasks, Task::new("t2", "second", vec!["t1".into()])).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn claim_sets_in_progress_and_owner() {
        let mut tasks = seed();
        claim_task(&mut tasks, "t1", "agent-pi").unwrap();
        assert_eq!(tasks[0].state, TaskState::InProgress);
        assert_eq!(tasks[0].claimed_by.as_deref(), Some("agent-pi"));
    }

    #[test]
    fn claim_blocked_until_dependency_completed() {
        let mut tasks = seed();
        add_task(&mut tasks, Task::new("t2", "second", vec!["t1".into()])).unwrap();
        // t2 blocked while t1 is still pending.
        let err = claim_task(&mut tasks, "t2", "agent-pi").unwrap_err();
        assert!(matches!(err, TeamError::BlockedByDependency { .. }));
        // Complete t1, then t2 unblocks.
        claim_task(&mut tasks, "t1", "agent-pi").unwrap();
        complete_task(&mut tasks, "t1", "agent-pi").unwrap();
        claim_task(&mut tasks, "t2", "agent-oracle").unwrap();
        assert_eq!(tasks[1].state, TaskState::InProgress);
    }

    #[test]
    fn cannot_claim_an_in_progress_task() {
        let mut tasks = seed();
        claim_task(&mut tasks, "t1", "agent-pi").unwrap();
        let err = claim_task(&mut tasks, "t1", "agent-oracle").unwrap_err();
        assert!(matches!(err, TeamError::NotClaimable { .. }));
    }

    #[test]
    fn only_claimant_completes() {
        let mut tasks = seed();
        claim_task(&mut tasks, "t1", "agent-pi").unwrap();
        let err = complete_task(&mut tasks, "t1", "agent-oracle").unwrap_err();
        assert!(matches!(err, TeamError::NotClaimant { .. }));
        complete_task(&mut tasks, "t1", "agent-pi").unwrap();
        assert_eq!(tasks[0].state, TaskState::Completed);
        assert!(tasks[0].completed_at.is_some());
    }

    #[test]
    fn cannot_complete_a_pending_task() {
        let mut tasks = seed();
        let err = complete_task(&mut tasks, "t1", "agent-pi").unwrap_err();
        assert!(matches!(err, TeamError::NotCompletable { .. }));
    }

    fn seed_plan_task() -> Vec<Task> {
        let mut t = Task::new("p1", "needs a plan", vec![]);
        t.requires_plan = true;
        vec![t]
    }

    #[test]
    fn plan_required_blocks_completion_until_approved() {
        let mut tasks = seed_plan_task();
        claim_task(&mut tasks, "p1", "agent-pi").unwrap();
        // No plan yet → complete refused.
        let err = complete_task(&mut tasks, "p1", "agent-pi").unwrap_err();
        assert!(matches!(err, TeamError::PlanNotApproved { .. }));
        // Submit a plan → still pending review → refused.
        submit_plan(&mut tasks, "p1", "agent-pi", "1. do X\n2. do Y").unwrap();
        assert!(matches!(
            complete_task(&mut tasks, "p1", "agent-pi").unwrap_err(),
            TeamError::PlanNotApproved { .. }
        ));
        // Approve → completion allowed.
        review_plan(&mut tasks, "p1", true).unwrap();
        complete_task(&mut tasks, "p1", "agent-pi").unwrap();
        assert_eq!(tasks[0].state, TaskState::Completed);
    }

    #[test]
    fn rejected_plan_can_be_resubmitted() {
        let mut tasks = seed_plan_task();
        claim_task(&mut tasks, "p1", "agent-pi").unwrap();
        submit_plan(&mut tasks, "p1", "agent-pi", "weak plan").unwrap();
        review_plan(&mut tasks, "p1", false).unwrap();
        assert_eq!(tasks[0].plan_approved, Some(false));
        // Resubmit clears the verdict back to pending.
        submit_plan(&mut tasks, "p1", "agent-pi", "stronger plan").unwrap();
        assert_eq!(tasks[0].plan_approved, None);
        assert_eq!(tasks[0].plan.as_deref(), Some("stronger plan"));
        review_plan(&mut tasks, "p1", true).unwrap();
        complete_task(&mut tasks, "p1", "agent-pi").unwrap();
        assert_eq!(tasks[0].state, TaskState::Completed);
    }

    #[test]
    fn cannot_submit_plan_for_unclaimed_or_others_task() {
        let mut tasks = seed_plan_task();
        // Unclaimed (pending) → NotClaimedForPlan.
        assert!(matches!(
            submit_plan(&mut tasks, "p1", "agent-pi", "x").unwrap_err(),
            TeamError::NotClaimedForPlan { .. }
        ));
        // Claimed by pi; oracle can't submit.
        claim_task(&mut tasks, "p1", "agent-pi").unwrap();
        assert!(matches!(
            submit_plan(&mut tasks, "p1", "agent-oracle", "x").unwrap_err(),
            TeamError::NotClaimant { .. }
        ));
    }

    #[test]
    fn cannot_review_a_plan_that_was_never_submitted() {
        let mut tasks = seed_plan_task();
        claim_task(&mut tasks, "p1", "agent-pi").unwrap();
        assert!(matches!(
            review_plan(&mut tasks, "p1", true).unwrap_err(),
            TeamError::NoPlanSubmitted { .. }
        ));
    }

    #[test]
    fn non_plan_task_completes_without_plan() {
        // requires_plan defaults false → existing behavior unchanged.
        let mut tasks = seed();
        claim_task(&mut tasks, "t1", "agent-pi").unwrap();
        complete_task(&mut tasks, "t1", "agent-pi").unwrap();
        assert_eq!(tasks[0].state, TaskState::Completed);
    }

    #[test]
    fn jsonl_round_trip_preserves_state() {
        let mut tasks = seed();
        add_task(&mut tasks, Task::new("t2", "second", vec!["t1".into()])).unwrap();
        claim_task(&mut tasks, "t1", "agent-pi").unwrap();
        let rendered = render_tasks(&tasks).unwrap();
        let parsed = parse_tasks(&rendered).unwrap();
        assert_eq!(parsed, tasks);
    }

    #[test]
    fn team_toml_round_trip() {
        let team = Team::new("squad", vec!["agent-pi".into(), "agent-oracle".into()]);
        let toml = team.to_toml().unwrap();
        let back = Team::from_toml(&toml).unwrap();
        assert_eq!(back, team);
        assert!(back.has_member("agent-pi"));
        assert!(!back.has_member("agent-ghost"));
    }

    #[test]
    fn ensure_member_rejects_outsider() {
        let team = Team::new("squad", vec!["agent-pi".into()]);
        assert!(ensure_member(&team, "agent-pi").is_ok());
        assert!(matches!(
            ensure_member(&team, "agent-x").unwrap_err(),
            TeamError::NotAMember(_)
        ));
    }

    #[test]
    fn team_reviewer_is_optional_and_roundtrips() {
        // Default: no reviewer, and the field stays out of the serialized toml.
        let plain = Team::new("squad", vec!["agent-pi".into()]);
        assert!(plain.reviewer.is_none());
        assert!(!plain.to_toml().unwrap().contains("reviewer"));

        // With a reviewer set, it roundtrips through toml.
        let mut t = Team::new("squad", vec!["agent-pi".into(), "agent-yanluo".into()]);
        t.reviewer = Some("agent-yanluo".into());
        let back = Team::from_toml(&t.to_toml().unwrap()).unwrap();
        assert_eq!(back.reviewer.as_deref(), Some("agent-yanluo"));
        assert_eq!(back, t);
    }

    #[test]
    fn team_chat_line_roundtrips() {
        let m = TeamChatMessage::new("agent-pi", "found the bug in the parser");
        let line = m.to_line().unwrap();
        assert!(!line.contains('\n'), "to_line is a single JSONL line");
        let back: TeamChatMessage = serde_json::from_str(&line).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn parse_chat_skips_blanks_and_preserves_order() {
        let body = format!(
            "{}\n\n{}\n",
            TeamChatMessage::new("agent-a", "first").to_line().unwrap(),
            TeamChatMessage::new("agent-b", "second").to_line().unwrap(),
        );
        let msgs = parse_chat(&body).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].from, "agent-a");
        assert_eq!(msgs[1].text, "second");
    }

    #[test]
    fn parse_chat_rejects_malformed_line() {
        assert!(matches!(
            parse_chat("{not json}\n").unwrap_err(),
            TeamError::Parse(_)
        ));
    }

    #[test]
    fn render_chat_then_parse_chat_round_trip() {
        let msgs = vec![
            TeamChatMessage::new("agent-a", "alpha"),
            TeamChatMessage::new("agent-b", "beta"),
        ];
        let back = parse_chat(&render_chat(&msgs).unwrap()).unwrap();
        assert_eq!(back, msgs);
    }

    // ── reopen ───────────────────────────────────────────────────────────────

    fn completed_list() -> Vec<Task> {
        let mut tasks = Vec::new();
        add_task(&mut tasks, Task::new("t1", "merge the branch", vec![])).unwrap();
        add_task(
            &mut tasks,
            Task::new("t2", "build on the merge", vec!["t1".to_string()]),
        )
        .unwrap();
        claim_task(&mut tasks, "t1", "agent-a").unwrap();
        complete_task(&mut tasks, "t1", "agent-a").unwrap();
        tasks
    }

    #[test]
    fn reopen_withdraws_the_completion_and_its_claim() {
        let mut tasks = completed_list();
        reopen_task(&mut tasks, "t1").unwrap();
        let t1 = tasks.iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(t1.state, TaskState::Pending);
        assert_eq!(
            t1.claimed_by, None,
            "a withdrawn completion has no claimant"
        );
        assert_eq!(t1.completed_at, None);
    }

    #[test]
    fn a_dependent_becomes_unclaimable_again() {
        // The whole point: a false completion unblocks downstream work, and
        // withdrawing it has to re-block that work.
        let mut tasks = completed_list();
        assert!(claim_task(&mut tasks, "t2", "agent-b").is_ok());

        // Reset t2 so the dependency check is what decides.
        let mut tasks = completed_list();
        reopen_task(&mut tasks, "t1").unwrap();
        let err = claim_task(&mut tasks, "t2", "agent-b").unwrap_err();
        assert!(
            matches!(err, TeamError::BlockedByDependency { ref dep, .. } if dep == "t1"),
            "{err}"
        );
    }

    #[test]
    fn reopening_a_plan_gated_task_clears_the_verdict() {
        // A surviving `Some(true)` would let the next claimant complete
        // without a fresh plan — the exact gate a false completion evades.
        let mut tasks = Vec::new();
        let mut t = Task::new("t1", "gated", vec![]);
        t.requires_plan = true;
        add_task(&mut tasks, t).unwrap();
        claim_task(&mut tasks, "t1", "agent-a").unwrap();
        submit_plan(&mut tasks, "t1", "agent-a", "the plan").unwrap();
        review_plan(&mut tasks, "t1", true).unwrap();
        complete_task(&mut tasks, "t1", "agent-a").unwrap();

        reopen_task(&mut tasks, "t1").unwrap();
        let t1 = tasks.iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(t1.plan_approved, None, "verdict must not survive a reopen");
        assert_eq!(
            t1.plan.as_deref(),
            Some("the plan"),
            "the claimant's own account is evidence, not a claim — keep it"
        );

        // And the gate really is armed again.
        claim_task(&mut tasks, "t1", "agent-b").unwrap();
        let err = complete_task(&mut tasks, "t1", "agent-b").unwrap_err();
        assert!(matches!(err, TeamError::PlanNotApproved { .. }), "{err}");
    }

    #[test]
    fn only_a_completed_task_can_be_reopened() {
        let mut tasks = Vec::new();
        add_task(&mut tasks, Task::new("t1", "x", vec![])).unwrap();
        assert!(matches!(
            reopen_task(&mut tasks, "t1").unwrap_err(),
            TeamError::NotReopenable { .. }
        ));
        claim_task(&mut tasks, "t1", "agent-a").unwrap();
        assert!(matches!(
            reopen_task(&mut tasks, "t1").unwrap_err(),
            TeamError::NotReopenable { .. }
        ));
        assert!(matches!(
            reopen_task(&mut tasks, "nope").unwrap_err(),
            TeamError::TaskNotFound(_)
        ));
    }

    #[test]
    fn reopen_is_idempotent_only_once() {
        // Reopening twice is an error, not a silent no-op: the second call
        // means the caller's model of the list is wrong.
        let mut tasks = completed_list();
        reopen_task(&mut tasks, "t1").unwrap();
        assert!(reopen_task(&mut tasks, "t1").is_err());
    }

    #[test]
    fn dependents_are_reported_not_cascaded() {
        let mut tasks = completed_list();
        assert_eq!(dependents_of(&tasks, "t1"), vec!["t2".to_string()]);
        reopen_task(&mut tasks, "t1").unwrap();
        // t2 is untouched — cascading would withdraw an assertion this call
        // was not asked to touch.
        let t2 = tasks.iter().find(|t| t.id == "t2").unwrap();
        assert_eq!(t2.state, TaskState::Pending);
        assert!(dependents_of(&tasks, "t2").is_empty());
    }
}
