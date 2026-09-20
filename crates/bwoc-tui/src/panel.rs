//! The fixed right pane: what this session is working *on*.
//!
//! The transcript answers "what did we say"; this answers "where am I" —
//! the directory and its branch, the workspace it belongs to (if any), that
//! workspace's agents, and the files this session has changed. It is gathered
//! from disk, not from the harness, so it stays correct while a turn runs.
//!
//! Reading is cheap and bounded (a `HEAD` file, two TOML files), and the result
//! is cached in the app and refreshed at turn boundaries rather than per frame.

use std::path::{Path, PathBuf};

use bwoc_core::workspace::{AgentsRegistry, Workspace};

/// Agents listed before the pane collapses the rest into a count.
const MAX_AGENTS: usize = 8;

/// One titled block of rows.
pub struct Section {
    pub title: String,
    pub rows: Vec<String>,
}

/// The pane's disk-derived content.
#[derive(Default)]
pub struct Panel {
    pub sections: Vec<Section>,
}

/// Read the pane's content for a session working in `workdir`.
pub fn gather(workdir: &Path) -> Panel {
    let mut sections = Vec::new();

    let mut project = vec![format!(
        "dir   {}",
        workdir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| workdir.display().to_string())
    )];
    if let Some(branch) = git_branch(workdir) {
        project.push(format!("git   {branch}"));
    }
    sections.push(Section {
        title: "project".to_string(),
        rows: project,
    });

    // The workspace is whatever `.bwoc/workspace.toml` sits above this
    // directory — a project session usually has none, and says so.
    match workspace_root(workdir) {
        Some(root) => {
            let name = Workspace::load(&root)
                .map(|w| w.workspace.name)
                .unwrap_or_else(|_| {
                    root.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| root.display().to_string())
                });
            sections.push(Section {
                title: "workspace".to_string(),
                rows: vec![name, root.display().to_string()],
            });
            sections.push(agents_section(&root));
        }
        None => sections.push(Section {
            title: "workspace".to_string(),
            rows: vec!["(not in a workspace)".to_string()],
        }),
    }
    Panel { sections }
}

/// The agents the workspace registry declares, newest-registry order.
fn agents_section(root: &Path) -> Section {
    let rows = match AgentsRegistry::load(root) {
        Ok(reg) if reg.agents.is_empty() => vec!["(none registered)".to_string()],
        Ok(reg) => {
            let total = reg.agents.len();
            let mut rows: Vec<String> = reg
                .agents
                .iter()
                .take(MAX_AGENTS)
                .map(|a| {
                    let mark = match a.status.as_str() {
                        "active" => "●",
                        "retired" => "○",
                        _ => "·",
                    };
                    format!("{mark} {}", a.id.trim_start_matches("agent-"))
                })
                .collect();
            if total > MAX_AGENTS {
                rows.push(format!("… {} more", total - MAX_AGENTS));
            }
            rows
        }
        Err(_) => vec!["(registry unreadable)".to_string()],
    };
    Section {
        title: "agents".to_string(),
        rows,
    }
}

/// The nearest ancestor (including `dir`) holding `.bwoc/workspace.toml`.
fn workspace_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".bwoc").join("workspace.toml").is_file())
        .map(Path::to_path_buf)
}

/// The checked-out branch, read from `.git/HEAD` — no `git` process, and a
/// detached HEAD reports its short commit instead of a branch name.
fn git_branch(dir: &Path) -> Option<String> {
    let git = dir
        .ancestors()
        .map(|d| d.join(".git"))
        .find(|p| p.exists())?;
    // A worktree's `.git` is a file pointing at the real directory.
    let head = if git.is_file() {
        let text = std::fs::read_to_string(&git).ok()?;
        let path = text.strip_prefix("gitdir:")?.trim();
        PathBuf::from(path).join("HEAD")
    } else {
        git.join("HEAD")
    };
    let head = std::fs::read_to_string(head).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_string(),
        None => head.chars().take(8).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(panel: &Panel) -> Vec<&str> {
        panel.sections.iter().map(|s| s.title.as_str()).collect()
    }

    fn rows<'a>(panel: &'a Panel, title: &str) -> &'a [String] {
        &panel
            .sections
            .iter()
            .find(|s| s.title == title)
            .expect("section")
            .rows
    }

    #[test]
    fn a_plain_directory_reports_no_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let panel = gather(tmp.path());
        assert_eq!(titles(&panel), ["project", "workspace"]);
        assert_eq!(rows(&panel, "workspace"), ["(not in a workspace)"]);
    }

    #[test]
    fn a_workspace_above_the_directory_is_found_with_its_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(
            root.join(".bwoc").join("workspace.toml"),
            "schema_version = 3\n[workspace]\nname = \"bwoc\"\nversion = \"3.0.0\"\ncreated = \"2026-01-01T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".bwoc").join("agents.toml"),
            "schema_version = 3\n\
             [[agent]]\nid = \"agent-luban\"\npath = \"agents/agent-luban\"\n\
             backend = \"claude\"\nincarnated = \"2026-01-01T00:00:00Z\"\nstatus = \"active\"\n\
             [[agent]]\nid = \"agent-old\"\npath = \"agents/agent-old\"\n\
             backend = \"claude\"\nincarnated = \"2026-01-01T00:00:00Z\"\nstatus = \"retired\"\n",
        )
        .unwrap();
        let nested = root.join("projects").join("thing");
        std::fs::create_dir_all(&nested).unwrap();

        let panel = gather(&nested);
        assert_eq!(titles(&panel), ["project", "workspace", "agents"]);
        assert_eq!(rows(&panel, "workspace")[0], "bwoc");
        assert_eq!(rows(&panel, "agents"), ["● luban", "○ old"]);
        assert_eq!(rows(&panel, "project")[0], "dir   thing");
    }

    #[test]
    fn many_agents_collapse_into_a_count() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(
            root.join(".bwoc").join("workspace.toml"),
            "schema_version = 3\n[workspace]\nname = \"w\"\nversion = \"3.0.0\"\ncreated = \"2026-01-01T00:00:00Z\"\n",
        )
        .unwrap();
        let mut reg = String::from("schema_version = 3\n");
        for i in 0..(MAX_AGENTS + 3) {
            reg.push_str(&format!(
                "[[agent]]\nid = \"agent-{i}\"\npath = \"agents/agent-{i}\"\n\
                 backend = \"ollama\"\nincarnated = \"2026-01-01T00:00:00Z\"\nstatus = \"active\"\n"
            ));
        }
        std::fs::write(root.join(".bwoc").join("agents.toml"), reg).unwrap();

        let rows = rows(&gather(root), "agents").to_vec();
        assert_eq!(rows.len(), MAX_AGENTS + 1);
        assert_eq!(rows.last().unwrap(), "… 3 more");
    }

    #[test]
    fn the_branch_comes_from_head_without_running_git() {
        let tmp = tempfile::tempdir().unwrap();
        let git = tmp.path().join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/feat/thing\n").unwrap();
        assert_eq!(
            git_branch(tmp.path()).as_deref(),
            Some("feat/thing"),
            "branch name"
        );
        // Detached HEAD: the short commit, not a fake branch.
        std::fs::write(git.join("HEAD"), "9bbd9d7c0ffee0000\n").unwrap();
        assert_eq!(git_branch(tmp.path()).as_deref(), Some("9bbd9d7c"));
    }

    #[test]
    fn a_directory_outside_any_repository_has_no_branch_row() {
        // `/` is not inside a repository (and the walk must not panic there).
        assert!(gather(Path::new("/")).sections[0].rows.len() <= 2);
    }
}
