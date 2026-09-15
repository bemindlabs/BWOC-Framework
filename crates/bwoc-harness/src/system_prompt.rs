//! System-prompt assembly for `--chat` and `--headless` sessions.
//!
//! The workdir decides the kind of session:
//!
//! - **Agent**: `config.manifest.json` is present (an incarnated agent, or a
//!   bwoc-connect public copy of one). The prompt is `AGENTS.md` (else
//!   `CLAUDE.md`) from the workdir, as before, followed by a condensed
//!   persona/mindsets block when the workdir has those slots. The caller still
//!   appends the Tier-1 `MEMORY.md` index.
//! - **Project**: anything else, e.g. bare `bwoc` in a repository. The prompt
//!   is the built-in coding preamble, an environment block, and the project
//!   instructions collected upward from the workdir to the git root.
//!
//! Public isolation: a workdir under `.bwoc/public/` never reads anything above
//! itself, whichever kind it is, so a stranger's session cannot pick up the
//! agent's (or its workspace's) instructions.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The project-session preamble. Backend-neutral; names the harness's tools.
pub const CODING_PREAMBLE: &str = include_str!("prompts/coding_agent.md");

/// Budget for all project `AGENTS.md` / `CLAUDE.md` text together.
pub const PROJECT_INSTRUCTIONS_CAP: usize = 32 * 1024;

/// Budget for the condensed persona/mindsets block in agent sessions.
pub const PERSONA_CAP: usize = 8 * 1024;

/// Longest a mindset's summary paragraph may be before it is cut.
const MINDSET_SUMMARY_CAP: usize = 400;

/// Each `git` call gets this long before it is killed.
const GIT_TIMEOUT: Duration = Duration::from_secs(2);

/// Per directory, the first of these that exists and is non-empty is used.
const INSTRUCTION_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Agent,
    Project,
}

pub fn session_kind(workdir: &Path) -> SessionKind {
    if workdir.join("config.manifest.json").is_file() {
        SessionKind::Agent
    } else {
        SessionKind::Project
    }
}

/// Is `workdir` inside a bwoc-connect public session directory
/// (`<agent>/.bwoc/public/<platform>-<chat>/`)?
pub fn is_public_workdir(workdir: &Path) -> bool {
    let parts: Vec<_> = workdir.components().map(|c| c.as_os_str()).collect();
    parts
        .windows(2)
        .any(|w| w[0] == ".bwoc" && w[1] == "public")
}

/// Build the system prompt for a chat or headless session in `workdir`.
pub fn assemble_chat_prompt(workdir: &Path) -> String {
    assemble_with_git(workdir, "git")
}

/// [`assemble_chat_prompt`] with the `git` executable injectable for tests.
pub fn assemble_with_git(workdir: &Path, git: &str) -> String {
    match session_kind(workdir) {
        SessionKind::Agent => {
            let mut prompt = workdir_instructions(workdir);
            if let Some(block) = persona_block(workdir) {
                prompt.push_str(&block);
            }
            prompt
        }
        SessionKind::Project => {
            let mut prompt = CODING_PREAMBLE.trim_end().to_string();
            if !is_public_workdir(workdir) {
                prompt.push_str("\n\n");
                prompt.push_str(&environment_block(workdir, git));
            }
            if let Some(block) = project_instructions_block(workdir, PROJECT_INSTRUCTIONS_CAP) {
                prompt.push_str("\n\n");
                prompt.push_str(&block);
            }
            prompt.push('\n');
            prompt
        }
    }
}

/// `AGENTS.md`, else `CLAUDE.md`, in `workdir` only. Empty when neither exists.
pub fn workdir_instructions(workdir: &Path) -> String {
    INSTRUCTION_FILES
        .iter()
        .find_map(|f| std::fs::read_to_string(workdir.join(f)).ok())
        .unwrap_or_default()
}

/// Directories whose instructions apply, nearest first: the workdir, then each
/// parent up to and including the nearest one holding `.git` (or up to the
/// filesystem root outside git). A public workdir yields only itself.
pub fn instruction_dirs(workdir: &Path) -> Vec<PathBuf> {
    if is_public_workdir(workdir) {
        return vec![workdir.to_path_buf()];
    }
    let mut dirs = Vec::new();
    for dir in workdir.ancestors() {
        dirs.push(dir.to_path_buf());
        if dir.join(".git").exists() {
            break;
        }
    }
    dirs
}

/// `(path, text)` per directory from [`instruction_dirs`], nearest first.
pub fn collect_instructions(workdir: &Path) -> Vec<(PathBuf, String)> {
    instruction_dirs(workdir)
        .into_iter()
        .filter_map(|dir| {
            INSTRUCTION_FILES.iter().find_map(|f| {
                let path = dir.join(f);
                std::fs::read_to_string(&path)
                    .ok()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| (path, t))
            })
        })
        .collect()
}

/// The `# Project instructions` block, root first and nearest last, within
/// `cap` bytes. When over budget the farthest directories are cut first, and a
/// note says how much was dropped. `None` when no file was found.
pub fn project_instructions_block(workdir: &Path, cap: usize) -> Option<String> {
    let files = collect_instructions(workdir);
    if files.is_empty() {
        return None;
    }
    let mut budget = cap;
    let mut omitted = 0usize;
    let mut kept = Vec::new();
    for (path, text) in files {
        if text.len() <= budget {
            budget -= text.len();
            kept.push((path, text));
        } else {
            let head = truncate_to(&text, budget);
            omitted += text.len() - head.len();
            budget = 0;
            if !head.trim().is_empty() {
                kept.push((path, head.to_string()));
            }
        }
    }
    kept.reverse();

    let mut out = String::from(
        "# Project instructions\n\n\
         From AGENTS.md / CLAUDE.md files between the repository root and the working \
         directory. The nearest file comes last and wins on conflict.\n",
    );
    if omitted > 0 {
        out.push_str(&format!(
            "\n(Truncated: {omitted} bytes omitted, farthest directories first, to fit {} KB.)\n",
            cap / 1024
        ));
    }
    for (path, text) in kept {
        out.push_str(&format!("\n## {}\n\n{}\n", path.display(), text.trim_end()));
    }
    Some(out)
}

/// Working directory, platform, UTC date and git state.
pub fn environment_block(workdir: &Path, git: &str) -> String {
    let now = bwoc_core::time::utc_now_iso8601();
    let date = now.get(..10).unwrap_or(&now);
    format!(
        "# Environment\n\n\
         - Working directory: {}\n\
         - Platform: {}/{}\n\
         - Date (UTC): {date}\n\
         - Git: {}\n",
        workdir.display(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        git_summary(workdir, git)
    )
}

/// One line of git state. Never fails: a missing or hung `git` just means less
/// detail.
pub fn git_summary(workdir: &Path, git: &str) -> String {
    if !workdir.ancestors().any(|d| d.join(".git").exists()) {
        return "not a git repository".to_string();
    }
    let branch = run_git(git, workdir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .map(|b| format!("branch `{}`", b.trim()))
        .or_else(|| {
            run_git(git, workdir, &["rev-parse", "--short", "HEAD"])
                .map(|h| format!("detached at `{}`", h.trim()))
        });
    let Some(branch) = branch else {
        return "repository (branch and status unknown: git unavailable)".to_string();
    };
    let state = match run_git(git, workdir, &["status", "--porcelain"]) {
        Some(s) if s.trim().is_empty() => "clean",
        Some(_) => "uncommitted changes",
        None => "status unknown",
    };
    format!("repository, {branch}, {state}")
}

/// Run `git -C dir args…`; stdout on success, `None` on any failure or timeout.
fn run_git(git: &str, dir: &Path, args: &[&str]) -> Option<String> {
    let mut child = Command::new(git)
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Drain on a thread so a large `status` cannot fill the pipe and stall git.
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut s);
        s
    });
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().ok()?;
    status.filter(|s| s.success()).map(|_| out)
}

/// Condensed persona and mindsets for an agent workdir, within [`PERSONA_CAP`]:
/// the `persona/README.md` body, then each `mindsets/*.md` (except the
/// `SPEC.md` / `README.md` templates) as title plus first paragraph.
/// Frontmatter is dropped. `None` when the agent has neither slot.
pub fn persona_block(agent_dir: &Path) -> Option<String> {
    let persona = std::fs::read_to_string(agent_dir.join("persona").join("README.md"))
        .ok()
        .map(|t| strip_frontmatter(&t).trim().to_string())
        .filter(|t| !t.is_empty());
    let mindsets = mindset_summaries(&agent_dir.join("mindsets"));
    if persona.is_none() && mindsets.is_empty() {
        return None;
    }

    let mut body = String::new();
    if let Some(p) = persona {
        body.push_str("## Persona\n\n");
        body.push_str(&p);
        body.push_str("\n\n");
    }
    if !mindsets.is_empty() {
        body.push_str("## Mindsets\n\n");
        for (title, summary) in mindsets {
            if summary.is_empty() {
                body.push_str(&format!("- **{title}**\n"));
            } else {
                body.push_str(&format!("- **{title}** — {summary}\n"));
            }
        }
    }
    let body = body.trim_end();
    let body = if body.len() > PERSONA_CAP {
        format!(
            "{}\n\n(Truncated to {} KB. The full text is in persona/ and mindsets/.)",
            truncate_to(body, PERSONA_CAP).trim_end(),
            PERSONA_CAP / 1024
        )
    } else {
        body.to_string()
    };
    Some(format!(
        "\n\n# Persona and mindsets (condensed)\n\n{body}\n"
    ))
}

/// `(title, first paragraph)` per mindset file, sorted by file name.
fn mindset_summaries(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            name != "spec.md" && name != "readme.md"
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            Some(summarize_mindset(&text, &stem))
        })
        .collect()
}

/// Title = first `# ` heading, else `stem`. Summary = the first paragraph that
/// is not a heading, with Obsidian callout markers removed, capped.
fn summarize_mindset(text: &str, stem: &str) -> (String, String) {
    let body = strip_frontmatter(text);
    let title = body
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| stem.to_string());

    let mut para: Vec<String> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            if para.is_empty() {
                continue;
            }
            break;
        }
        let mut l = line.trim_start_matches('>').trim_start();
        if l.starts_with("[!") {
            l = l
                .split_once(']')
                .map(|(_, rest)| rest.trim_start())
                .unwrap_or(l);
        }
        if !l.is_empty() {
            para.push(l.to_string());
        }
    }
    let summary = para.join(" ");
    let summary = if summary.len() > MINDSET_SUMMARY_CAP {
        format!("{}…", truncate_to(&summary, MINDSET_SUMMARY_CAP).trim_end())
    } else {
        summary
    };
    (title, summary)
}

/// Text after a leading `---` … `---` YAML block; the whole text otherwise.
fn strip_frontmatter(text: &str) -> &str {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            return &rest[offset..];
        }
    }
    text
}

/// The longest prefix of `s` no longer than `max` bytes that ends on a char
/// boundary.
fn truncate_to(s: &str, max: usize) -> &str {
    let mut i = max.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn project_walk_is_root_first_nearest_last_and_stops_at_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let app = repo.join("pkg").join("app");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&app).unwrap();
        write(&tmp.path().join("AGENTS.md"), "OUTSIDE-THE-REPO");
        write(&repo.join("AGENTS.md"), "ROOT-RULES");
        write(&repo.join("pkg").join("CLAUDE.md"), "PKG-RULES");
        write(&app.join("AGENTS.md"), "APP-RULES");
        write(&app.join("CLAUDE.md"), "SHADOWED-BY-AGENTS");

        let prompt = assemble_with_git(&app, "bwoc-test-no-such-git");
        let root = prompt.find("ROOT-RULES").expect("root file");
        let pkg = prompt.find("PKG-RULES").expect("CLAUDE.md fallback");
        let nearest = prompt.find("APP-RULES").expect("nearest file");
        assert!(root < pkg && pkg < nearest, "order wrong:\n{prompt}");
        assert!(!prompt.contains("OUTSIDE-THE-REPO"));
        assert!(!prompt.contains("SHADOWED-BY-AGENTS"));
        assert!(prompt.starts_with("# You are a coding agent"));
        assert!(
            prompt.find("# Environment").unwrap() < prompt.find("# Project instructions").unwrap()
        );
    }

    #[test]
    fn project_instructions_cap_cuts_farthest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let sub = repo.join("sub");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        write(
            &repo.join("AGENTS.md"),
            &format!("ROOT{}", "r".repeat(3000)),
        );
        write(&sub.join("AGENTS.md"), &format!("NEAR{}", "n".repeat(3000)));

        let block = project_instructions_block(&sub, 4096).unwrap();
        assert!(
            block.contains(&format!("NEAR{}", "n".repeat(3000))),
            "nearest intact"
        );
        assert!(block.contains("ROOT"), "root partially kept");
        assert!(!block.contains(&"r".repeat(3000)), "root cut");
        // Nearest file: 3004 bytes. Root keeps the remaining 1092 of its 3004.
        assert!(block.contains("(Truncated: 1912 bytes omitted"), "{block}");
        let instruction_bytes = block.matches(['r', 'n']).count();
        assert!(instruction_bytes <= 4096 + 200);
    }

    #[test]
    fn nothing_found_means_no_instructions_block() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let prompt = assemble_with_git(tmp.path(), "bwoc-test-no-such-git");
        assert!(prompt.contains("# Environment"));
        assert!(!prompt.contains("# Project instructions"));
    }

    #[test]
    fn git_block_tolerates_missing_git() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        assert!(git_summary(tmp.path(), "bwoc-test-no-such-git").contains("git unavailable"));

        let env = environment_block(tmp.path(), "bwoc-test-no-such-git");
        assert!(env.contains(&format!("- Working directory: {}", tmp.path().display())));
        assert!(env.contains(std::env::consts::OS));
        assert!(env.contains("- Date (UTC): 20"));
    }

    #[test]
    fn git_block_outside_a_repository() {
        let tmp = tempfile::tempdir().unwrap();
        if tmp.path().ancestors().any(|d| d.join(".git").exists()) {
            return; // temp dir lives inside a checkout; nothing to assert
        }
        assert_eq!(git_summary(tmp.path(), "git"), "not a git repository");
    }

    #[test]
    fn git_block_reads_branch_and_dirty_state_when_git_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = Command::new("git")
            .args(["init", "-q", "-b", "trunk"])
            .current_dir(tmp.path())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return; // git not installed: the missing-git test covers this path
        }
        assert_eq!(
            git_summary(tmp.path(), "git"),
            "repository, branch `trunk`, clean"
        );
        write(&tmp.path().join("new.txt"), "x");
        assert_eq!(
            git_summary(tmp.path(), "git"),
            "repository, branch `trunk`, uncommitted changes"
        );
    }

    fn agent_dir(root: &Path) -> PathBuf {
        let agent = root.join("agents").join("agent-demo");
        write(&agent.join("config.manifest.json"), "{}");
        write(&agent.join("AGENTS.md"), "AGENT-PROFILE");
        write(
            &agent.join("persona").join("README.md"),
            "---\ntitle: Agent Persona\ntags:\n  - type/persona\n---\n\n# Agent Persona\n\nPERSONA-BODY\n",
        );
        write(&agent.join("mindsets").join("SPEC.md"), "SPEC-TEMPLATE");
        write(
            &agent.join("mindsets").join("b-second.md"),
            "---\ntitle: Second\n---\n\n# 🎯 Second Mindset\n\n> [!abstract] Patch narrowly,\n> nothing more.\n\n## When\n\n- LATER-SECTION\n",
        );
        write(
            &agent.join("mindsets").join("a-first.md"),
            "Plain first paragraph.\n\nNOT-FIRST\n",
        );
        agent
    }

    #[test]
    fn agent_mode_keeps_profile_and_adds_condensed_persona() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("AGENTS.md"), "WORKSPACE-PARENT");
        let agent = agent_dir(tmp.path());

        let prompt = assemble_with_git(&agent, "bwoc-test-no-such-git");
        assert!(prompt.starts_with("AGENT-PROFILE"), "{prompt}");
        assert!(!prompt.contains("You are a coding agent"));
        assert!(!prompt.contains("# Environment"));
        assert!(!prompt.contains("WORKSPACE-PARENT"));
        assert!(prompt.contains("# Persona and mindsets (condensed)"));
        assert!(prompt.contains("PERSONA-BODY"));
        assert!(!prompt.contains("title: Agent Persona"));
        assert!(!prompt.contains("SPEC-TEMPLATE"));
        assert!(prompt.contains("- **a-first** — Plain first paragraph."));
        assert!(prompt.contains("- **🎯 Second Mindset** — Patch narrowly, nothing more."));
        assert!(!prompt.contains("NOT-FIRST") && !prompt.contains("LATER-SECTION"));
        assert!(prompt.find("a-first").unwrap() < prompt.find("Second Mindset").unwrap());
    }

    #[test]
    fn agent_without_slots_is_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("config.manifest.json"), "{}");
        write(&tmp.path().join("AGENTS.md"), "ONLY-PROFILE");
        assert_eq!(assemble_with_git(tmp.path(), "git"), "ONLY-PROFILE");
    }

    #[test]
    fn persona_block_is_capped() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("persona").join("README.md"),
            &"ข้อความยาว ".repeat(2000),
        );
        let block = persona_block(tmp.path()).unwrap();
        assert!(block.len() <= PERSONA_CAP + 200, "{}", block.len());
        assert!(block.contains("(Truncated to 8 KB."));
    }

    #[test]
    fn public_workdir_never_reads_above_itself() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        write(&tmp.path().join("AGENTS.md"), "WORKSPACE-PARENT");
        let agent = agent_dir(tmp.path());
        write(&agent.join("CLAUDE.md"), "AGENT-PRIVATE");
        let public = agent.join(".bwoc").join("public").join("telegram-42");
        write(&public.join("AGENTS.md"), "PUBLIC-COPY");

        assert!(is_public_workdir(&public));
        assert!(!is_public_workdir(&agent));
        assert_eq!(instruction_dirs(&public), vec![public.clone()]);

        // As bwoc-connect prepares it: a copied manifest makes it an agent session.
        write(&public.join("config.manifest.json"), "{}");
        let as_agent = assemble_with_git(&public, "bwoc-test-no-such-git");
        assert_eq!(as_agent, "PUBLIC-COPY");

        // Even without the manifest, the project walk stays inside.
        std::fs::remove_file(public.join("config.manifest.json")).unwrap();
        let as_project = assemble_with_git(&public, "bwoc-test-no-such-git");
        assert!(as_project.contains("PUBLIC-COPY"));
        for leaked in [
            "AGENT-PROFILE",
            "AGENT-PRIVATE",
            "WORKSPACE-PARENT",
            "PERSONA-BODY",
        ] {
            assert!(
                !as_project.contains(leaked),
                "{leaked} leaked:\n{as_project}"
            );
        }
        assert!(!as_project.contains("# Environment"));
    }

    #[test]
    fn preamble_stays_short() {
        assert!(CODING_PREAMBLE.lines().count() <= 60);
    }
}
