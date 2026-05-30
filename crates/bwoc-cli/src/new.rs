//! `bwoc new` — incarnate a new agent from the template.
//!
//! Rust port of `modules/agent-template/scripts/incarnate.sh` with the
//! manifest-input behavior spec'd in `docs/en/INCARNATION.en.md` §"Setting
//! the Manifest". This iteration adds interactive TTY prompts for missing
//! required fields (non-TTY = fail-fast). Unix symlinks only (Windows
//! symlink handling deferred).

use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::{AgentEntry, AgentsRegistry, Workspace};
use include_dir::{Dir, include_dir};

use crate::i18n;
use crate::spawn::Backend;
use crate::util::utc_now_iso8601;

/// Agent template embedded into the binary at compile time. Used as the
/// final fallback when no on-disk template is found, so `bwoc new` works
/// from any directory after a `cargo install`.
static EMBEDDED_TEMPLATE: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../../modules/agent-template");

/// Arguments collected from the CLI for `bwoc new`. Required fields are
/// `Option<String>` because they can be filled by interactive prompt when
/// stdin is a TTY.
pub struct NewArgs {
    pub name: String,
    pub target: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub backend: Backend,
    pub lang: String,
    // Required at incarnation time; promptable.
    pub role: Option<String>,
    pub primary_model: Option<String>,
    pub lint_cmd: Option<String>,
    pub format_cmd: Option<String>,
    pub test_cmd: Option<String>,
    pub build_cmd: Option<String>,
    // Truly optional.
    pub fallback_model: Option<String>,
    pub memory_path: String,
    pub sessions_path: Option<String>,
    pub deep_memory_cmd: Option<String>,
    pub worktree_base: Option<String>,
    /// Persona scope: 1-line "this agent does X". Fills `{{scopeDescription}}`.
    pub scope: Option<String>,
    /// Persona anti-scope: 1-line "this agent does NOT do Y". Fills `{{outOfScope}}`.
    pub out_of_scope: Option<String>,
    /// Primary capability: longer description of what this agent is skilled at.
    /// Fills `{{primaryCapability}}`. Defaults to the role value when not provided.
    pub primary_capability: Option<String>,
    /// Comma-separated names of initial mindsets — one stub `.md` per name.
    pub mindsets: Option<String>,
    /// Comma-separated names of initial skills — one stub `.md` per name.
    pub skills: Option<String>,
    /// Emit JSON `{ agent_id, target, registered_in, symlinks, mindset_stubs,
    /// skill_stubs, persona_filled }` instead of the human-readable
    /// incarnation report. Useful for scripted multi-agent setup.
    pub json: bool,
}

/// All required fields resolved to concrete strings; ready for incarnate.
struct Resolved {
    name: String,
    target: PathBuf,
    template: PathBuf,
    backend: Backend,
    role: String,
    primary_model: String,
    fallback_model: Option<String>,
    memory_path: String,
    sessions_path: Option<String>,
    deep_memory_cmd: Option<String>,
    lint_cmd: String,
    format_cmd: String,
    test_cmd: String,
    build_cmd: String,
    worktree_base: Option<String>,
    scope_description: Option<String>,
    out_of_scope: Option<String>,
    /// Fills `{{primaryCapability}}`. Defaults to `role` when not supplied.
    primary_capability: Option<String>,
    mindsets: Vec<String>,
    skills: Vec<String>,
}

/// Entry point — returns the process exit code.
pub fn run(args: NewArgs) -> i32 {
    let bundle = i18n::bundle_for(&args.lang);
    let json = args.json;
    match incarnate(args, &bundle) {
        Ok(report) => {
            if json {
                let value = serde_json::json!({
                    "agent_id": report.agent_id,
                    "target": report.target.display().to_string(),
                    "registered_in": report.registered_in
                        .as_ref()
                        .map(|p| p.display().to_string()),
                    "symlinks": report.symlinks,
                    "mindset_stubs": report.mindset_stubs,
                    "skill_stubs": report.skill_stubs,
                    "persona_filled": report.persona_filled,
                });
                println!(
                    "{}",
                    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                print_report(&report, &bundle);
            }
            0
        }
        Err(e) => {
            // Error path stays English (thiserror localization deferred).
            eprintln!("bwoc new: {e}");
            match e {
                NewError::InvalidName(_)
                | NewError::TargetExists(_)
                | NewError::TemplateNotFound
                | NewError::MissingFields(_) => 2,
                _ => 1,
            }
        }
    }
}

/// Successful incarnation result; printed at the end of `bwoc new`.
pub struct IncarnationReport {
    pub agent_id: String,
    pub target: PathBuf,
    pub symlinks: Vec<String>,
    /// Workspace root that the new agent was registered to, if any.
    pub registered_in: Option<PathBuf>,
    /// Mindset stub files created under `mindsets/` (relative paths).
    pub mindset_stubs: Vec<String>,
    /// Skill stub files created under `skills/` (relative paths).
    pub skill_stubs: Vec<String>,
    /// Whether persona scope placeholders got substituted (vs left for manual edit).
    pub persona_filled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum NewError {
    #[error("invalid agent name '{0}' — must be lowercase kebab-case (a-z, 0-9, -)")]
    InvalidName(String),
    #[error("target path already exists: {0}")]
    TargetExists(PathBuf),
    #[error(
        "template not found — pass --template <path> or run from a directory under the framework"
    )]
    TemplateNotFound,
    #[error(
        "missing required field(s) and stdin is not a TTY (cannot prompt): {}",
        .0.join(", ")
    )]
    MissingFields(Vec<String>),
    #[error("agent id '{agent_id}' already registered in workspace {workspace}")]
    DuplicateRegistration {
        agent_id: String,
        workspace: PathBuf,
    },
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("manifest error: {0}")]
    Manifest(#[from] bwoc_core::manifest::ManifestError),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
    #[error("symlink failure for {file}: {source}")]
    Symlink {
        file: String,
        #[source]
        source: io::Error,
    },
}

/// Perform the full incarnation atomically (best-effort — if a step fails
/// after copy, the partial target remains for the user to inspect).
pub fn incarnate(
    args: NewArgs,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) -> Result<IncarnationReport, NewError> {
    validate_name(&args.name)?;
    let resolved = resolve(args, bundle)?;

    if resolved.target.exists() {
        return Err(NewError::TargetExists(resolved.target));
    }

    // 1. Copy template tree to target (skip .git, *.example.*).
    copy_tree(&resolved.template, &resolved.target)?;

    // 2. Create backend symlinks (replace any copied files first).
    let symlinks = create_symlinks(&resolved.target)?;

    // 3. Build and write the resolved manifest.
    let manifest = build_manifest(&resolved);
    let manifest_path = resolved.target.join("config.manifest.json");
    manifest.save_to_path(&manifest_path)?;

    // 4. Substitute all manifest-backed placeholders in AGENTS.md +
    //    persona/README.md so the freshly-incarnated agent passes `bwoc check`
    //    without any manual editing. The only placeholder left raw is
    //    `{{taskId}}`, which is resolved at task-assignment time (runtime).
    let persona_filled = resolved.scope_description.is_some() || resolved.out_of_scope.is_some();
    substitute_all_placeholders(&resolved.target, &resolved)?;

    // 5. Seed user-requested mindset and skill stubs (each is a stub `.md`
    //    file under mindsets/<name>.md or skills/<name>.md; user fills the
    //    actual content later).
    let mindset_stubs = seed_mindset_stubs(&resolved.target, &resolved.mindsets)?;
    let skill_stubs = seed_skill_stubs(&resolved.target, &resolved.skills)?;

    // 6. Register with the nearest workspace if one is found.
    let registered_in = match register_in_workspace(&resolved.target, &manifest, resolved.backend) {
        Ok(ws) => ws,
        Err(e) => {
            // Registration is best-effort: log but don't fail the incarnation,
            // since the agent files are already on disk and valid.
            eprintln!("bwoc new: warning — agent created but workspace registration failed: {e}");
            None
        }
    };

    Ok(IncarnationReport {
        agent_id: manifest.agent_id,
        target: resolved.target,
        symlinks,
        registered_in,
        mindset_stubs,
        skill_stubs,
        persona_filled,
    })
}

/// Walk up from `target.parent()` looking for `.bwoc/workspace.toml`. If
/// found, load `agents.toml`, append an entry for the new agent, and save.
/// Returns the workspace root on success; `None` if no workspace exists.
fn register_in_workspace(
    target: &Path,
    manifest: &Manifest,
    backend: Backend,
) -> Result<Option<PathBuf>, NewError> {
    let Some(workspace_root) = find_workspace(target) else {
        return Ok(None);
    };

    let mut registry = AgentsRegistry::load(&workspace_root)?;

    // Refuse to register a duplicate id; the user can `bwoc retire` first.
    if registry.agents.iter().any(|a| a.id == manifest.agent_id) {
        return Err(NewError::DuplicateRegistration {
            agent_id: manifest.agent_id.clone(),
            workspace: workspace_root,
        });
    }

    let rel_path = target
        .strip_prefix(&workspace_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| target.display().to_string());

    registry.agents.push(AgentEntry {
        id: manifest.agent_id.clone(),
        path: rel_path,
        backend: backend.display_name().to_string(),
        incarnated: utc_now_iso8601(),
        status: "active".to_string(),
    });
    registry.save(&workspace_root)?;
    Ok(Some(workspace_root))
}

/// Walk up from `target.parent()` looking for an ancestor directory that
/// contains `.bwoc/workspace.toml`. Returns the workspace root if found.
fn find_workspace(target: &Path) -> Option<PathBuf> {
    let mut cur = target.parent()?.to_path_buf();
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn validate_name(name: &str) -> Result<(), NewError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err(NewError::InvalidName(name.to_string()));
    }
    Ok(())
}

fn resolve_template(explicit: Option<&Path>) -> Result<PathBuf, NewError> {
    // 1. Explicit --template flag wins.
    if let Some(p) = explicit {
        if !p.is_dir() {
            return Err(NewError::TemplateNotFound);
        }
        return Ok(p.to_path_buf());
    }
    // 2. `BWOC_TEMPLATE` env var (advanced user override).
    if let Some(env) = std::env::var_os("BWOC_TEMPLATE") {
        let p = PathBuf::from(env);
        if p.is_dir() && p.join("AGENTS.md").is_file() {
            return Ok(p);
        }
    }
    // 3. Ancestor walk for `modules/agent-template/` (framework-developer path).
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = cwd;
        loop {
            let candidate = cur.join("modules/agent-template");
            if candidate.is_dir() && candidate.join("AGENTS.md").is_file() {
                return Ok(candidate);
            }
            if !cur.pop() {
                break;
            }
        }
    }
    // 4. `~/.bwoc/template/` (pre-populated cache; survives across invocations).
    if let Some(home) = std::env::var_os("HOME") {
        let cached = PathBuf::from(home).join(".bwoc/template");
        if cached.is_dir() && cached.join("AGENTS.md").is_file() {
            return Ok(cached);
        }
    }
    // 5. Final fallback: extract the embedded template to a pid-tagged tmp dir.
    // This is what makes `bwoc new` work from any directory after `cargo install`.
    let tmp = std::env::temp_dir().join(format!("bwoc-template-{}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;
    EMBEDDED_TEMPLATE.extract(&tmp).map_err(|e| {
        NewError::Io(io::Error::other(format!(
            "failed to extract embedded template: {e}"
        )))
    })?;
    Ok(tmp)
}

fn default_target(template: &Path, name: &str) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // 1. Workspace-aware (highest priority): if cwd is inside a BWOC
    //    workspace, place the new agent at
    //    `<workspace_root>/<defaults.agents_dir>/agent-<name>` per
    //    WORKSPACE.en.md. Even when running from inside the framework
    //    repo (which is itself a workspace), this wins — the previous
    //    "framework-developer sibling" branch placed agents OUTSIDE
    //    `agents/` and required users to manually `mv` after, then
    //    left the registry pointing at the wrong relative path.
    if let Some(ws_root) = find_workspace_root_from(&cwd) {
        let agents_dir = Workspace::load(&ws_root)
            .map(|w| w.defaults.agents_dir)
            .unwrap_or_else(|_| "agents".to_string());
        return ws_root.join(agents_dir).join(format!("agent-{name}"));
    }

    // 2. No workspace anywhere; template lives under
    //    `modules/agent-template/` — drop the new agent next to it.
    //    Useful when scaffolding inside a fresh framework clone before
    //    `bwoc init` has been run.
    if template.ends_with("modules/agent-template")
        && let Some(p) = template.parent().and_then(|p| p.parent())
    {
        return p.join(format!("agent-{name}"));
    }

    // 3. Last resort: cwd/agent-<name>.
    cwd.join(format!("agent-{name}"))
}

/// Walk up from `start` looking for `.bwoc/workspace.toml`. Unlike
/// `find_workspace` (which starts from `target.parent()`), this starts
/// from any given path — used by `default_target` to consult the
/// enclosing workspace's `defaults.agents_dir`.
fn find_workspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Project stacks we recognise well enough to suggest lint/format/test/build
/// defaults for. Detected by looking for a manifest file in `cwd` or any
/// ancestor up to (and including) a `.bwoc/workspace.toml` boundary.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ProjectKind {
    Rust,
    Node,
    Python,
    Go,
    Unknown,
}

impl ProjectKind {
    /// User-facing name for the detected stack. Stays English (terms are
    /// the stack's own brand names — "Rust", "Node", etc.).
    fn display_name(self) -> &'static str {
        match self {
            ProjectKind::Rust => "Rust",
            ProjectKind::Node => "Node",
            ProjectKind::Python => "Python",
            ProjectKind::Go => "Go",
            ProjectKind::Unknown => "Unknown",
        }
    }
}

/// Detect the project kind from `start`, walking up to either a manifest
/// hit or the enclosing workspace root (we don't escape the workspace).
fn detect_project_kind(start: &Path) -> ProjectKind {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join("Cargo.toml").is_file() {
            return ProjectKind::Rust;
        }
        if cur.join("package.json").is_file() {
            return ProjectKind::Node;
        }
        if cur.join("pyproject.toml").is_file() || cur.join("requirements.txt").is_file() {
            return ProjectKind::Python;
        }
        if cur.join("go.mod").is_file() {
            return ProjectKind::Go;
        }
        // Don't escape the workspace boundary if we're inside one.
        if cur.join(".bwoc/workspace.toml").is_file() {
            return ProjectKind::Unknown;
        }
        if !cur.pop() {
            return ProjectKind::Unknown;
        }
    }
}

/// Suggested default command for one of the build-related manifest fields.
/// Returns `None` if the stack is `Unknown` or the field has no sensible
/// default for this stack (e.g. Python has no canonical `build`).
fn suggested_cmd(kind: ProjectKind, field: &str) -> Option<&'static str> {
    match (kind, field) {
        (ProjectKind::Rust, "lintCmd") => Some("cargo clippy --all-targets -- -D warnings"),
        (ProjectKind::Rust, "formatCmd") => Some("cargo fmt --all -- --check"),
        (ProjectKind::Rust, "testCmd") => Some("cargo test --workspace"),
        (ProjectKind::Rust, "buildCmd") => Some("cargo build --workspace"),

        (ProjectKind::Node, "lintCmd") => Some("npm run lint"),
        (ProjectKind::Node, "formatCmd") => Some("npm run format -- --check"),
        (ProjectKind::Node, "testCmd") => Some("npm test"),
        (ProjectKind::Node, "buildCmd") => Some("npm run build"),

        (ProjectKind::Python, "lintCmd") => Some("ruff check ."),
        (ProjectKind::Python, "formatCmd") => Some("ruff format --check ."),
        (ProjectKind::Python, "testCmd") => Some("pytest"),
        // No canonical Python build cmd — leave for the user.
        (ProjectKind::Python, "buildCmd") => None,

        (ProjectKind::Go, "lintCmd") => Some("go vet ./..."),
        (ProjectKind::Go, "formatCmd") => Some("gofmt -l ."),
        (ProjectKind::Go, "testCmd") => Some("go test ./..."),
        (ProjectKind::Go, "buildCmd") => Some("go build ./..."),

        _ => None,
    }
}

/// Fill in any missing required fields by interactive prompt (TTY) or fail
/// fast with the list of missing fields (non-TTY).
fn resolve(
    args: NewArgs,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) -> Result<Resolved, NewError> {
    let template = resolve_template(args.template.as_deref())?;
    let target = args
        .target
        .clone()
        .unwrap_or_else(|| default_target(&template, &args.name));

    let descriptions = read_descriptions(&template)?;
    let tty = io::stdin().is_terminal();

    // First pass — collect missing fields without prompting, so we can fail
    // fast with the complete list when stdin is not a TTY.
    if !tty {
        let mut missing = Vec::new();
        if args.role.is_none() {
            missing.push("agentRole".to_string());
        }
        if args.primary_model.is_none() {
            missing.push("primaryModel".to_string());
        }
        if args.lint_cmd.is_none() {
            missing.push("lintCmd".to_string());
        }
        if args.format_cmd.is_none() {
            missing.push("formatCmd".to_string());
        }
        if args.test_cmd.is_none() {
            missing.push("testCmd".to_string());
        }
        if args.build_cmd.is_none() {
            missing.push("buildCmd".to_string());
        }
        if !missing.is_empty() {
            return Err(NewError::MissingFields(missing));
        }
    }

    let role = resolve_role(args.role, &descriptions, tty, bundle)?;
    let primary_model =
        resolve_primary_model(args.primary_model, args.backend, &descriptions, tty, bundle)?;

    // Detect the project stack once and feed sensible defaults into the
    // four cmd prompts so the user can press Enter to accept them.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let kind = detect_project_kind(&cwd);
    // Surface what we detected so the user knows *why* the defaults look
    // the way they do (or why there aren't any). TTY-only — scripts don't
    // need the line.
    if tty {
        let stack_name = kind.display_name();
        let msg = if matches!(kind, ProjectKind::Unknown) {
            i18n::t(bundle, "new-detect-unknown")
        } else {
            i18n::t_with(bundle, "new-detect-stack", &[("stack", stack_name)])
        };
        println!("{msg}");
    }
    let lint_cmd = resolve_one(
        args.lint_cmd,
        "lintCmd",
        &descriptions,
        tty,
        bundle,
        suggested_cmd(kind, "lintCmd"),
    )?;
    let format_cmd = resolve_one(
        args.format_cmd,
        "formatCmd",
        &descriptions,
        tty,
        bundle,
        suggested_cmd(kind, "formatCmd"),
    )?;
    let test_cmd = resolve_one(
        args.test_cmd,
        "testCmd",
        &descriptions,
        tty,
        bundle,
        suggested_cmd(kind, "testCmd"),
    )?;
    let build_cmd = resolve_one(
        args.build_cmd,
        "buildCmd",
        &descriptions,
        tty,
        bundle,
        suggested_cmd(kind, "buildCmd"),
    )?;

    // Persona scope is optional but recommended — promptable on TTY.
    // Empty input leaves `{{scopeDescription}}` raw for manual edit later.
    let scope_description = resolve_optional_text(
        args.scope,
        "scope",
        "Persona scope — what does this agent DO? (one line; Enter to skip)",
        tty,
    )?;
    let out_of_scope = resolve_optional_text(
        args.out_of_scope,
        "outOfScope",
        "Persona anti-scope — what does it NOT do? (one line; Enter to skip)",
        tty,
    )?;
    let primary_capability = resolve_optional_text(
        args.primary_capability,
        "primaryCapability",
        "Primary capability — longer description of what this agent is skilled at (Enter to use role)",
        tty,
    )?;
    let mindsets = parse_comma_list(args.mindsets.as_deref())
        .or_else(|| {
            prompt_optional_csv(
                tty,
                "Initial mindsets to seed (comma-separated kebab-case; Enter to skip)",
            )
        })
        .unwrap_or_default();
    let skills = parse_comma_list(args.skills.as_deref())
        .or_else(|| {
            prompt_optional_csv(
                tty,
                "Initial skills to seed (comma-separated kebab-case; Enter to skip)",
            )
        })
        .unwrap_or_default();

    Ok(Resolved {
        name: args.name,
        target,
        template,
        backend: args.backend,
        role,
        primary_model,
        fallback_model: args.fallback_model,
        memory_path: args.memory_path,
        sessions_path: args.sessions_path,
        deep_memory_cmd: args.deep_memory_cmd,
        lint_cmd,
        format_cmd,
        test_cmd,
        build_cmd,
        worktree_base: args.worktree_base,
        scope_description,
        out_of_scope,
        primary_capability,
        mindsets,
        skills,
    })
}

/// Prompt for a single line of text on TTY; non-TTY without `cur` returns None.
/// Empty input returns None (the placeholder stays raw in the cloned files).
fn resolve_optional_text(
    cur: Option<String>,
    _key: &str,
    prompt_text: &str,
    tty: bool,
) -> Result<Option<String>, NewError> {
    if let Some(v) = cur {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        return Ok(Some(trimmed.to_string()));
    }
    if !tty {
        return Ok(None);
    }
    use std::io::{Write as _, stdin, stdout};
    let mut out = stdout();
    write!(out, "{prompt_text}: ")?;
    out.flush()?;
    let mut line = String::new();
    stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

/// Parse `--mindsets "a,b,c"` (CLI flag form) into Some(["a","b","c"]).
/// None when the flag wasn't passed; empty Vec when passed but empty.
fn parse_comma_list(raw: Option<&str>) -> Option<Vec<String>> {
    let s = raw?;
    Some(
        s.split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
    )
}

/// TTY-only prompt for a comma-separated list. Returns Some(...) when the
/// user gave any non-empty input, None otherwise (so callers can chain).
fn prompt_optional_csv(tty: bool, prompt_text: &str) -> Option<Vec<String>> {
    if !tty {
        return None;
    }
    use std::io::{Write as _, stdin, stdout};
    let mut out = stdout();
    let _ = write!(out, "{prompt_text}: ");
    let _ = out.flush();
    let mut line = String::new();
    if stdin().read_line(&mut line).is_err() {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
    )
}

/// Either return the already-supplied value, or interactively prompt
/// (assumed to only be called when `tty` is true after fail-fast guard).
///
/// `suggestion` is an optional pre-filled default. When set, the prompt
/// shows it as `[default: ...]` and an empty input accepts it instead of
/// erroring. When `None`, empty input is rejected as before.
fn resolve_one(
    cur: Option<String>,
    key: &str,
    descriptions: &HashMap<String, String>,
    tty: bool,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
    suggestion: Option<&str>,
) -> Result<String, NewError> {
    if let Some(v) = cur {
        return Ok(v);
    }
    if !tty {
        // Defensive: fail-fast guard should have caught this earlier.
        return Err(NewError::MissingFields(vec![key.to_string()]));
    }
    let desc = descriptions
        .get(key)
        .map(|s| s.as_str())
        .unwrap_or("required field");
    let prompt = match suggestion {
        Some(s) => i18n::t_with(
            bundle,
            "new-prompt-format-with-default",
            &[("key", key), ("desc", desc), ("default", s)],
        ),
        None => i18n::t_with(bundle, "new-prompt-format", &[("key", key), ("desc", desc)]),
    };
    let mut stdout = io::stdout();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if trimmed.is_empty() {
        // Enter on a prompt with a suggestion → accept the suggestion.
        if let Some(s) = suggestion {
            return Ok(s.to_string());
        }
        return Err(NewError::MissingFields(vec![key.to_string()]));
    }
    Ok(trimmed.to_string())
}

/// Specialized prompt for `primaryModel` — shows a numbered list of common
/// models for the chosen `backend` so the user can pick by number, by typed
/// name, or by hitting Enter to accept the first (recommended) entry.
fn resolve_primary_model(
    cur: Option<String>,
    backend: Backend,
    descriptions: &HashMap<String, String>,
    tty: bool,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) -> Result<String, NewError> {
    if let Some(v) = cur {
        return Ok(v);
    }
    if !tty {
        return Err(NewError::MissingFields(vec!["primaryModel".to_string()]));
    }

    let models = backend.models();
    let mut stdout = io::stdout();

    // Print the picker header + numbered list.
    let header = i18n::t_with(
        bundle,
        "new-model-picker-header",
        &[("backend", backend.display_name())],
    );
    let default_hint = i18n::t(bundle, "new-model-picker-default-hint");
    writeln!(stdout, "{header}")?;
    for (i, m) in models.iter().enumerate() {
        if i == 0 {
            writeln!(stdout, "  {}. {m}  {default_hint}", i + 1)?;
        } else {
            writeln!(stdout, "  {}. {m}", i + 1)?;
        }
    }

    // Use the standard key (desc) prompt format for consistency with other fields.
    let desc = descriptions
        .get("primaryModel")
        .map(|s| s.as_str())
        .unwrap_or("required field");
    let prompt = i18n::t_with(
        bundle,
        "new-prompt-format",
        &[("key", "primaryModel"), ("desc", desc)],
    );
    write!(stdout, "{prompt}")?;
    stdout.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();

    // Empty input → take the first (recommended) entry.
    if trimmed.is_empty() {
        return Ok(models[0].to_string());
    }
    // Pure number that matches a list index → resolve to that model.
    if let Ok(n) = trimmed.parse::<usize>()
        && n >= 1
        && n <= models.len()
    {
        return Ok(models[n - 1].to_string());
    }
    // Otherwise treat as a custom model name (free-text fallback).
    Ok(trimmed.to_string())
}

/// Common agent roles offered in the `bwoc new` picker. First entry is
/// the recommended default. Free-text input is always accepted —
/// this is a convenience, not a whitelist. The role string ends up
/// verbatim in the manifest's `agentRole` field, so keep them concise
/// (one or two words, not a sentence).
const AGENT_ROLE_SUGGESTIONS: &[&str] = &[
    "code reviewer",
    "documentation writer",
    "test author",
    "refactoring helper",
    "onboarding assistant",
    "migration specialist",
];

/// Specialized prompt for `agentRole` — shows a numbered list of common
/// roles so the user can pick by number, by typed phrase, or by hitting
/// Enter to accept the first (recommended) entry. Mirror of
/// `resolve_primary_model` for consistency.
fn resolve_role(
    cur: Option<String>,
    descriptions: &HashMap<String, String>,
    tty: bool,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) -> Result<String, NewError> {
    if let Some(v) = cur {
        return Ok(v);
    }
    if !tty {
        return Err(NewError::MissingFields(vec!["agentRole".to_string()]));
    }

    let roles = AGENT_ROLE_SUGGESTIONS;
    let mut stdout = io::stdout();

    let header = i18n::t(bundle, "new-role-picker-header");
    let default_hint = i18n::t(bundle, "new-model-picker-default-hint");
    writeln!(stdout, "{header}")?;
    for (i, r) in roles.iter().enumerate() {
        if i == 0 {
            writeln!(stdout, "  {}. {r}  {default_hint}", i + 1)?;
        } else {
            writeln!(stdout, "  {}. {r}", i + 1)?;
        }
    }

    let desc = descriptions
        .get("agentRole")
        .map(|s| s.as_str())
        .unwrap_or("required field");
    let prompt = i18n::t_with(
        bundle,
        "new-prompt-format",
        &[("key", "agentRole"), ("desc", desc)],
    );
    write!(stdout, "{prompt}")?;
    stdout.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return Ok(roles[0].to_string());
    }
    if let Ok(n) = trimmed.parse::<usize>()
        && n >= 1
        && n <= roles.len()
    {
        return Ok(roles[n - 1].to_string());
    }
    Ok(trimmed.to_string())
}

/// Read the template's `config.manifest.json` and return a map of
/// `requiredConfig.<field>.description` strings. Errors propagate as
/// `NewError::Io` / `NewError::Manifest`.
fn read_descriptions(template: &Path) -> Result<HashMap<String, String>, NewError> {
    let manifest_path = template.join("config.manifest.json");
    let content = fs::read_to_string(&manifest_path)?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(bwoc_core::manifest::ManifestError::from)?;
    let mut out = HashMap::new();
    if let Some(rc) = v.get("requiredConfig").and_then(|x| x.as_object()) {
        for (key, schema) in rc {
            if let Some(desc) = schema.get("description").and_then(|d| d.as_str()) {
                out.insert(key.clone(), desc.to_string());
            }
        }
    }
    Ok(out)
}

/// Recursive copy. Skips `.git/` and `*.example.*` to match the shell script.
fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".git" {
            continue;
        }
        if name_str.contains(".example.") {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_tree(&from, &to)?;
        } else if ft.is_symlink() {
            // Preserve symlinks rather than dereferencing them.
            #[cfg(unix)]
            {
                let target = fs::read_link(&from)?;
                let _ = fs::remove_file(&to);
                std::os::unix::fs::symlink(&target, &to)?;
            }
            #[cfg(not(unix))]
            {
                // On non-Unix, copy the link target's content rather than the link.
                fs::copy(&from, &to)?;
            }
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Force-create `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md` → `AGENTS.md`
/// in the target directory. Removes any pre-existing file/symlink first.
fn create_symlinks(target: &Path) -> Result<Vec<String>, NewError> {
    let backends = ["CLAUDE.md", "AGY.md", "CODEX.md", "KIMI.md", "OLLAMA.md"];
    let mut created = Vec::new();
    for backend in backends {
        let p = target.join(backend);
        let _ = fs::remove_file(&p);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("AGENTS.md", &p).map_err(|e| NewError::Symlink {
                file: backend.to_string(),
                source: e,
            })?;
        }
        #[cfg(not(unix))]
        {
            // Windows symlinks require special privileges; defer per spec.
            return Err(NewError::Symlink {
                file: backend.to_string(),
                source: io::Error::new(
                    io::ErrorKind::Other,
                    "Windows symlink support deferred to Phase 2",
                ),
            });
        }
        created.push(format!("{backend} -> AGENTS.md"));
    }
    Ok(created)
}

fn build_manifest(r: &Resolved) -> Manifest {
    Manifest {
        name: r.name.clone(),
        agent_id: format!("agent-{}", r.name),
        agent_role: r.role.clone(),
        primary_model: r.primary_model.clone(),
        fallback_model: r.fallback_model.clone(),
        // `bwoc new` does not expose an --auto-models flag; operators opt into
        // `primaryModel: "auto"` by hand-editing the manifest's `autoModels`.
        auto_models: None,
        // No --reasoning-effort flag either; set in the manifest by hand.
        reasoning_effort: None,
        memory_path: r.memory_path.clone(),
        sessions_path: r.sessions_path.clone(),
        deep_memory_cmd: r.deep_memory_cmd.clone(),
        lint_cmd: r.lint_cmd.clone(),
        format_cmd: r.format_cmd.clone(),
        test_cmd: r.test_cmd.clone(),
        build_cmd: r.build_cmd.clone(),
        worktree_base: r.worktree_base.clone(),
        scope_description: r.scope_description.clone(),
        out_of_scope: r.out_of_scope.clone(),
        // Trust spec v1 ships permissive-by-default at the framework
        // level; the scaffolding floor (requiredTrust = [vatta,
        // noCatthana]) for new agents will land in a follow-up step.
        // For now, `bwoc new` writes no trust block — equivalent to
        // "no qualities declared, no gating" per the spec.
        trust: None,
        backend: None,
        base_url: None,
        version: "2.0".to_string(),
    }
}

/// Substitute every manifest-backed placeholder in the freshly-cloned
/// agent's AGENTS.md and persona/README.md. After this runs, the only
/// `{{...}}` patterns remaining must be `{{taskId}}` (runtime) so that
/// `bwoc check` reports zero violations on a brand-new incarnation.
///
/// Rules for optional fields:
/// - `{{fallbackModel}}`  → empty string when not provided (config block becomes `""`)
/// - `{{deepMemoryCmd}}`  → `# (Tier 2 not configured)` when not provided
/// - `{{worktreeBase}}`   → `/tmp` when not provided
/// - `{{primaryCapability}}` → `role` value when not provided
/// - `{{moduleName}}`     → `<module>` (shows it is a per-task fill-in, not an
///   incarnation constant; angle brackets signal an example)
/// - `{{maxConcurrentTasks}}` → `3` (the spec default from config.manifest.json)
fn substitute_all_placeholders(target: &Path, r: &Resolved) -> Result<(), NewError> {
    let agent_id = format!("agent-{}", r.name);
    let scope = r.scope_description.as_deref().unwrap_or("");
    let out_of_scope = r.out_of_scope.as_deref().unwrap_or("");
    let primary_capability = r
        .primary_capability
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&r.role);
    let fallback_model = r.fallback_model.as_deref().unwrap_or("");
    let deep_memory_cmd = r
        .deep_memory_cmd
        .as_deref()
        .unwrap_or("# (Tier 2 not configured)");
    let worktree_base = r.worktree_base.as_deref().unwrap_or("/tmp");

    // Ordered pairs: (placeholder, replacement). Applied to every target file.
    let substitutions: &[(&str, &str)] = &[
        ("{{agentId}}", &agent_id),
        ("{{name}}", &r.name),
        ("{{agentRole}}", &r.role),
        ("{{primaryCapability}}", primary_capability),
        ("{{primaryModel}}", &r.primary_model),
        ("{{fallbackModel}}", fallback_model),
        ("{{memoryPath}}", &r.memory_path),
        ("{{deepMemoryCmd}}", deep_memory_cmd),
        ("{{lintCmd}}", &r.lint_cmd),
        ("{{formatCmd}}", &r.format_cmd),
        ("{{testCmd}}", &r.test_cmd),
        ("{{buildCmd}}", &r.build_cmd),
        ("{{worktreeBase}}", worktree_base),
        ("{{scopeDescription}}", scope),
        ("{{outOfScope}}", out_of_scope),
        // Documentation-example placeholders:
        // moduleName is per-task (filled at task time), shown in section 2.2 example.
        // maxConcurrentTasks is the spec default (3); not stored in the Manifest struct.
        ("{{moduleName}}", "<module>"),
        ("{{maxConcurrentTasks}}", "3"),
    ];

    for rel in ["AGENTS.md", "persona/README.md"] {
        let path = target.join(rel);
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let mut updated = content.clone();
        for (placeholder, replacement) in substitutions {
            updated = updated.replace(placeholder, replacement);
        }
        if updated != content {
            std::fs::write(&path, &updated)?;
        }
    }
    Ok(())
}

/// Write empty-but-structured stub files into mindsets/ for each name
/// supplied. Names are kebab-case slugs. Existing files are NOT clobbered.
fn seed_mindset_stubs(target: &Path, names: &[String]) -> Result<Vec<String>, NewError> {
    seed_stubs(target, names, "mindsets", |name| {
        format!(
            "---\ntitle: {title}\naliases: []\ntags:\n  - type/mindset\n  - principle/TODO\n---\n\n# {title}\n\n> [!abstract] One-sentence description of when this mindset applies.\n\n## When to Apply\n\nTODO\n\n## How to Apply\n\nTODO\n\n## When NOT to Apply\n\nTODO\n\n## Related Principles\n\n- TODO (link to PHILOSOPHY.en.md)\n",
            title = title_case(name)
        )
    })
}

/// Write empty-but-structured stub files into skills/ for each name.
fn seed_skill_stubs(target: &Path, names: &[String]) -> Result<Vec<String>, NewError> {
    seed_stubs(target, names, "skills", |name| {
        format!(
            "---\ntitle: {title}\naliases: []\ntags:\n  - type/skill\n  - domain/TODO\nmaturity: L1\n---\n\n# {title}\n\n> [!abstract] One-sentence description of what the agent does with this skill.\n\n## Domain\n\nTODO\n\n## Inputs\n\nTODO\n\n## Outputs\n\nTODO\n\n## Verification Gates\n\nTODO\n\n## Out of Scope\n\nTODO\n",
            title = title_case(name)
        )
    })
}

fn seed_stubs(
    target: &Path,
    names: &[String],
    subdir: &str,
    body_for: impl Fn(&str) -> String,
) -> Result<Vec<String>, NewError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let dir = target.join(subdir);
    std::fs::create_dir_all(&dir)?;
    let mut created = Vec::new();
    for raw in names {
        let slug = raw.trim();
        if slug.is_empty() {
            continue;
        }
        let path = dir.join(format!("{slug}.md"));
        if path.exists() {
            continue; // don't clobber
        }
        std::fs::write(&path, body_for(slug))?;
        created.push(format!("{subdir}/{slug}.md"));
    }
    Ok(created)
}

/// "verify-before-act" → "Verify Before Act"
fn title_case(slug: &str) -> String {
    slug.split('-')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_report(
    report: &IncarnationReport,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) {
    let target_display = report.target.display().to_string();

    println!();
    println!(
        "{}",
        i18n::t_with(
            bundle,
            "new-report-incarnated",
            &[("agent_id", &report.agent_id)]
        )
    );
    println!(
        "{}",
        i18n::t_with(bundle, "new-report-target", &[("path", &target_display)])
    );
    println!();
    for s in &report.symlinks {
        // Symlink lines are data ("CLAUDE.md -> AGENTS.md"); no localization.
        println!("+ {s}");
    }

    // Persona substitution + mindset/skill seeding feedback. These only
    // fire when the user actually supplied --scope/--out-of-scope/--mindsets
    // /--skills (or answered the TTY prompts) — silent otherwise to keep
    // the default output untouched.
    if report.persona_filled {
        println!();
        println!("Persona scope: substituted into AGENTS.md + persona/README.md");
    }
    if !report.mindset_stubs.is_empty() {
        println!();
        println!("Mindset stubs seeded:");
        for s in &report.mindset_stubs {
            println!("+ {s}");
        }
    }
    if !report.skill_stubs.is_empty() {
        println!();
        println!("Skill stubs seeded:");
        for s in &report.skill_stubs {
            println!("+ {s}");
        }
    }

    println!();
    match &report.registered_in {
        Some(ws) => {
            let ws_display = ws.display().to_string();
            println!(
                "{}",
                i18n::t_with(bundle, "new-report-registered", &[("path", &ws_display)])
            );
        }
        None => {
            println!("{}", i18n::t(bundle, "new-report-not-registered"));
        }
    }
    println!();
    println!("{}", i18n::t(bundle, "new-report-next-steps-header"));
    println!(
        "  {}",
        i18n::t_with(
            bundle,
            "new-report-step-check",
            &[("path", &target_display)]
        )
    );
    println!("  {}", i18n::t(bundle, "new-report-step-edit-agents"));
    println!("  {}", i18n::t(bundle, "new-report-step-edit-persona"));
    println!("  {}", i18n::t(bundle, "new-report-step-git"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with_role_only() -> NewArgs {
        NewArgs {
            name: "demo".to_string(),
            target: None,
            template: None,
            backend: Backend::Claude,
            lang: "en".to_string(),
            role: Some("tester".to_string()),
            primary_model: None,
            lint_cmd: None,
            format_cmd: None,
            test_cmd: None,
            build_cmd: None,
            fallback_model: None,
            memory_path: "memories/".to_string(),
            sessions_path: None,
            deep_memory_cmd: None,
            worktree_base: None,
            scope: None,
            out_of_scope: None,
            primary_capability: None,
            mindsets: None,
            skills: None,
            json: false,
        }
    }

    #[test]
    fn detect_and_suggest() {
        // Build a tmp dir for each known manifest file → assert detection.
        let base = std::env::temp_dir().join(format!("bwoc-detect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        for (manifest, expected) in [
            ("Cargo.toml", ProjectKind::Rust),
            ("package.json", ProjectKind::Node),
            ("pyproject.toml", ProjectKind::Python),
            ("go.mod", ProjectKind::Go),
        ] {
            let dir = base.join(manifest.replace('.', "_"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(manifest), "x").unwrap();
            assert_eq!(detect_project_kind(&dir), expected, "for {manifest}");
        }
        // A bare dir with nothing → Unknown.
        let bare = base.join("bare");
        fs::create_dir_all(&bare).unwrap();
        assert_eq!(detect_project_kind(&bare), ProjectKind::Unknown);
        let _ = fs::remove_dir_all(&base);

        // Catalog spot-checks: representative entries per stack + the gap.
        assert_eq!(
            suggested_cmd(ProjectKind::Rust, "lintCmd"),
            Some("cargo clippy --all-targets -- -D warnings"),
        );
        assert_eq!(
            suggested_cmd(ProjectKind::Node, "testCmd"),
            Some("npm test")
        );
        assert_eq!(
            suggested_cmd(ProjectKind::Go, "buildCmd"),
            Some("go build ./..."),
        );
        // Python has no canonical build cmd by design — must return None.
        assert_eq!(suggested_cmd(ProjectKind::Python, "buildCmd"), None);
        // Unknown stack must always return None.
        assert_eq!(suggested_cmd(ProjectKind::Unknown, "lintCmd"), None);
    }

    #[test]
    fn name_validation() {
        assert!(validate_name("agent-foo").is_ok());
        assert!(validate_name("foo").is_ok());
        assert!(validate_name("agent-foo-2").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("Agent").is_err());
        assert!(validate_name("agent_foo").is_err());
        assert!(validate_name("-foo").is_err());
        assert!(validate_name("foo-").is_err());
    }

    #[test]
    fn build_manifest_carries_required_fields() {
        let r = Resolved {
            name: "demo".to_string(),
            target: PathBuf::from("/tmp/demo"),
            template: PathBuf::from("/template"),
            backend: Backend::Claude,
            role: "tester".to_string(),
            primary_model: "model-x".to_string(),
            fallback_model: Some("model-y".to_string()),
            memory_path: "memories/".to_string(),
            sessions_path: None,
            deep_memory_cmd: None,
            lint_cmd: "true".to_string(),
            format_cmd: "true".to_string(),
            test_cmd: "true".to_string(),
            build_cmd: "true".to_string(),
            worktree_base: None,
            scope_description: None,
            out_of_scope: None,
            primary_capability: None,
            mindsets: Vec::new(),
            skills: Vec::new(),
        };
        let m = build_manifest(&r);
        assert_eq!(m.agent_id, "agent-demo");
        assert_eq!(m.primary_model, "model-x");
        assert_eq!(m.fallback_model.as_deref(), Some("model-y"));
    }

    #[test]
    fn find_workspace_walks_ancestors() {
        // Build a temp tree:
        //   /tmp/bwoc-new-test-<pid>/ws/.bwoc/workspace.toml
        //   /tmp/bwoc-new-test-<pid>/ws/agents/agent-foo  (the target)
        let mut root = std::env::temp_dir();
        root.push(format!("bwoc-new-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ws/.bwoc")).unwrap();
        std::fs::write(
            root.join("ws/.bwoc/workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\ncreated=\"x\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("ws/agents/agent-foo")).unwrap();

        let target = root.join("ws/agents/agent-foo");
        let ws = find_workspace(&target);
        assert_eq!(ws.as_deref(), Some(root.join("ws").as_path()));

        // Non-workspace path returns None.
        let lone = root.join("ws/agents/agent-foo/sub/dir");
        std::fs::create_dir_all(&lone).unwrap();
        // The .bwoc/ at /ws is still an ancestor → still finds it.
        assert!(find_workspace(&lone).is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Construct a fresh non-TTY check: `resolve` must collect all missing
    /// required fields into `MissingFields` rather than blocking on stdin.
    /// We can't change is_terminal() in-process; instead exercise the
    /// fail-fast logic directly by calling `resolve` here — `cargo test`
    /// runs with stdin redirected from /dev/null, so `is_terminal()` is
    /// false during tests.
    #[test]
    fn non_tty_missing_required_fields_fails_fast() {
        let args = args_with_role_only();
        // Provide a template that has the config.manifest.json so
        // `resolve_template` succeeds when --template is None and we're
        // running from the framework root. cargo test sets CWD = the
        // crate directory by default, so use an explicit template path.
        let mut args = args;
        args.template =
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/agent-template"));
        let bundle = i18n::bundle_for("en");
        match resolve(args, &bundle) {
            Err(NewError::MissingFields(fields)) => {
                // role is supplied; the other 5 are missing.
                assert!(!fields.contains(&"agentRole".to_string()));
                assert!(fields.contains(&"primaryModel".to_string()));
                assert!(fields.contains(&"lintCmd".to_string()));
                assert!(fields.contains(&"formatCmd".to_string()));
                assert!(fields.contains(&"testCmd".to_string()));
                assert!(fields.contains(&"buildCmd".to_string()));
            }
            Ok(_) => panic!("expected MissingFields error, got Ok"),
            Err(e) => panic!("expected MissingFields, got {e:?}"),
        }
    }

    #[test]
    fn descriptions_loaded_from_template() {
        let template =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/agent-template");
        let descs = read_descriptions(&template).expect("template should be readable");
        assert!(descs.contains_key("agentRole"));
        assert!(descs.contains_key("primaryModel"));
        assert!(descs.contains_key("lintCmd"));
    }

    // ---- End-to-end: incarnate + check no unsubstituted placeholders ----------
    // Verifies the fix for GitHub issue #4: `bwoc new` must substitute all
    // manifest-backed placeholders so `bwoc check` passes with zero violations
    // on a freshly-incarnated agent (except the runtime `{{taskId}}`).
    //
    // Unix-only: `incarnate` calls `create_symlinks` which requires Unix symlinks.

    #[cfg(unix)]
    #[test]
    fn incarnate_leaves_no_unsubstituted_placeholders() {
        use crate::check::{audit, extract_placeholders};

        let template =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/agent-template");
        // Skip if template doesn't exist (e.g. partial checkout).
        if !template.join("AGENTS.md").is_file() {
            return;
        }

        let target =
            std::env::temp_dir().join(format!("bwoc-incarnate-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&target);

        let args = NewArgs {
            name: "scribe".to_string(),
            target: Some(target.clone()),
            template: Some(template),
            backend: Backend::Claude,
            lang: "en".to_string(),
            role: Some("documentation writer".to_string()),
            primary_model: Some("claude-opus-4-7".to_string()),
            fallback_model: Some("claude-sonnet-4-6".to_string()),
            lint_cmd: Some("cargo clippy --all-targets -- -D warnings".to_string()),
            format_cmd: Some("cargo fmt --all -- --check".to_string()),
            test_cmd: Some("cargo test --workspace".to_string()),
            build_cmd: Some("cargo build --workspace".to_string()),
            memory_path: "memories/".to_string(),
            sessions_path: None,
            deep_memory_cmd: None,
            worktree_base: Some("/tmp".to_string()),
            scope: Some("writes and maintains documentation".to_string()),
            out_of_scope: Some("does not write production code".to_string()),
            primary_capability: Some(
                "technical writing; doc review; changelog maintenance".to_string(),
            ),
            mindsets: None,
            skills: None,
            json: false,
        };

        let bundle = i18n::bundle_for("en");
        incarnate(args, &bundle).expect("incarnate should succeed");

        // Read the incarnated AGENTS.md and assert no unsubstituted {{...}}
        // placeholders remain (except the runtime {{taskId}}).
        let agents_md = target.join("AGENTS.md");
        assert!(
            agents_md.is_file(),
            "AGENTS.md must exist after incarnation"
        );
        let content = fs::read_to_string(&agents_md).expect("AGENTS.md readable");

        let found = extract_placeholders(&content);
        let runtime_only: &[&str] = &["{{taskId}}"];
        let unsubstituted: Vec<&String> = found
            .iter()
            .filter(|ph| !runtime_only.contains(&ph.as_str()))
            .collect();

        assert!(
            unsubstituted.is_empty(),
            "AGENTS.md has unsubstituted placeholders after incarnation: {:?}",
            unsubstituted
        );

        // Also run the full audit in incarnation mode and assert zero violations.
        let report = audit(&target);
        let placeholder_violations: Vec<&String> = report
            .violations
            .iter()
            .filter(|v| v.contains("unsubstituted placeholder"))
            .collect();
        assert!(
            placeholder_violations.is_empty(),
            "bwoc check found placeholder violations: {:?}",
            placeholder_violations
        );

        let _ = fs::remove_dir_all(&target);
    }
}
