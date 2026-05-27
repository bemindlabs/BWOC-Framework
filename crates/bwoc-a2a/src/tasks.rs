//! A2A `tasks/*` ↔ BWOC Saṅgha team tasks (#48 **P2**).
//!
//! Bridges a team's shared task list (`bwoc_core::team`) to the A2A task model
//! so an external A2A client can query what a BWOC team is working on. The
//! mapping is deliberately lossy-but-honest: BWOC's task machine has three
//! states (`Pending → InProgress → Completed`), so only the three matching A2A
//! states are ever produced. The other A2A states (`INPUT_REQUIRED`,
//! `AUTH_REQUIRED`, `FAILED`, `CANCELED`, `REJECTED`) have no BWOC equivalent
//! and are never synthesized — and BWOC tasks are **not A2A-cancelable** (the
//! human lead owns the lifecycle), which the `CancelTask` handler surfaces
//! rather than faking.

use std::path::Path;

use bwoc_core::team::{self, Task as TeamTask, TaskState as TeamState};

use crate::types::{Task as A2aTask, TaskState as A2aState, TaskStatus};

/// Map a BWOC team task state onto its A2A counterpart. Total over BWOC's
/// three states; the remaining A2A states are intentionally unreachable here.
pub fn a2a_state(state: TeamState) -> A2aState {
    match state {
        TeamState::Pending => A2aState::Submitted,
        TeamState::InProgress => A2aState::Working,
        TeamState::Completed => A2aState::Completed,
    }
}

/// Render one BWOC team task as an A2A [`Task`](A2aTask). The team id is the A2A
/// `contextId` — the shared list is the context the task lives in.
pub fn to_a2a_task(task: &TeamTask, team_id: &str) -> A2aTask {
    A2aTask {
        id: task.id.clone(),
        context_id: team_id.to_string(),
        status: TaskStatus {
            state: a2a_state(task.state),
        },
    }
}

/// Load a team's task list from its `tasks.jsonl`. A missing file is an empty
/// list (a team with no tasks yet), not an error.
pub fn load_team_tasks(tasks_path: &Path) -> Result<Vec<TeamTask>, team::TeamError> {
    match std::fs::read_to_string(tasks_path) {
        Ok(body) => team::parse_tasks(&body),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(team::TeamError::Parse(format!(
            "reading {}: {e}",
            tasks_path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_map_to_the_three_corresponding_a2a_states() {
        assert_eq!(a2a_state(TeamState::Pending), A2aState::Submitted);
        assert_eq!(a2a_state(TeamState::InProgress), A2aState::Working);
        assert_eq!(a2a_state(TeamState::Completed), A2aState::Completed);
    }

    #[test]
    fn team_task_renders_with_team_as_context_id() {
        let t = TeamTask::new("t1", "harden the listener", vec![]);
        let a = to_a2a_task(&t, "team-security");
        assert_eq!(a.id, "t1");
        assert_eq!(a.context_id, "team-security");
        assert_eq!(a.status.state, A2aState::Submitted); // new → pending → submitted
    }

    #[test]
    fn missing_tasks_file_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let got = load_team_tasks(&dir.path().join("nope/tasks.jsonl")).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn loads_and_maps_a_real_task_list() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tasks.jsonl");
        let mut tasks = vec![TeamTask::new("t1", "first", vec![])];
        tasks.push(TeamTask::new("t2", "second", vec![]));
        tasks[1].state = TeamState::Completed;
        std::fs::write(&p, team::render_tasks(&tasks).unwrap()).unwrap();

        let loaded = load_team_tasks(&p).unwrap();
        assert_eq!(loaded.len(), 2);
        let mapped: Vec<_> = loaded.iter().map(|t| to_a2a_task(t, "team-x")).collect();
        assert_eq!(mapped[0].status.state, A2aState::Submitted);
        assert_eq!(mapped[1].status.state, A2aState::Completed);
    }
}
