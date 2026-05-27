//! `bwoc` — the BWOC framework CLI.
//!
//! Phase 1 v2.0. Implemented subcommands so far: `check`, `new`, `spawn`.
//! Others land in follow-up fires of the loop. See `crates/bwoc-cli/README.md`
//! for the full surface and per-command status.

use clap::{Args, Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

mod a2a;
mod audit;
mod banner;
mod chat;
mod check;
mod completion;
mod dashboard;
mod deep_memory_cmd;
mod doc_cmd;
mod doctor;
mod fleet;
mod git_worktree;
mod help;
mod i18n;
mod inbox;
mod init;
mod jira;
mod livecheck;
mod log;
mod memory;
mod new;
mod peer;
mod ping;
mod plugin;
mod retire;
mod run;
mod sangha;
mod send;
mod sessions;
mod skill;
mod spawn;
mod start;
mod status;
mod stop;
mod supervise;
mod trust;
mod update;
mod user_home;
mod util;
mod whats_new;
mod workspace;

#[derive(Parser, Debug)]
#[command(
    name = "bwoc",
    version,
    about = "BWOC — Buddhist Way of Coding agent framework CLI.",
    long_about = "Phase 1 v2.0. See modules/agent-template/docs/en/PHILOSOPHY.en.md §0.1 The Arc.",
    // We provide our own `help` subcommand (topical guides). Disable clap's
    // auto-generated one to avoid the duplicate-name conflict.
    disable_help_subcommand = true
)]
struct Cli {
    /// Language for CLI output. Phase 1 ships with `en` and `th`.
    /// Precedence: --lang flag > BWOC_LANG env > $LANG > en fallback.
    #[arg(long, global = true)]
    lang: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Verify backend neutrality of an agent or the template (read-only audit).
    Check {
        /// Path to the agent or template to audit. Defaults to current directory.
        /// Mutually exclusive with `--all`.
        #[arg(conflicts_with = "all")]
        path: Option<PathBuf>,
        /// Audit every incarnated agent in the workspace (fleet-wide audit).
        /// Mutually exclusive with `<path>`.
        #[arg(long)]
        all: bool,
        /// Workspace root (only used with `--all`). Defaults follow standard
        /// resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Emit JSON to stdout instead of the human-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Incarnate a new agent from the template (uppāda).
    New(Box<NewArgs>),
    /// Exec the configured LLM backend CLI in an agent's directory (uppāda → ṭhiti).
    Spawn(SpawnArgs),
    /// Initialize a BWOC workspace at the given path (uppāda).
    Init(InitArgs),
    /// Inspect a BWOC workspace.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// List agents registered in the enclosing workspace's agents.toml.
    List(ListArgs),
    /// Diagnose environment + workspace; with `--auto`, fix safe issues in place. `--json` for structured output.
    Doctor(DoctorArgs),
    /// Retire an agent — remove it from the workspace's registry (vaya).
    Retire(RetireArgs),
    /// Show per-agent health + identity snapshot (read-only).
    Status(StatusArgs),
    /// Topic-specific help (backends, workspace, manifest, arc, getting-started).
    Help(HelpArgs),
    /// Emit a shell completion script (bash, zsh, fish, powershell, elvish).
    Completion(CompletionArgs),
    /// Launch the interactive TUI dashboard (agents list with navigation; refresh with `r`).
    Dashboard(DashboardArgs),
    /// Pause an agent — set status = "stopped" without removing files.
    Stop(StopArgs),
    /// Reactivate a stopped agent — set status = "active".
    Start(StartArgs),
    /// Ping a `bwoc-agent --serve`'d agent over its Unix socket (PING → PONG).
    Ping(PingArgs),
    /// Supervise an agent's daemon — restart on crash, exit cleanly when stopped.
    Supervise(SuperviseArgs),
    /// Read an agent's Kalyāṇamitta-7 trust profile (declared + requiredTrust).
    Trust(TrustArgs),
    /// Append a message to an agent's inbox (`.bwoc/inbox.jsonl`).
    Send(SendArgs),
    /// Chat with an agent — exec backend CLI with manifest-driven model.
    Chat(ChatArgs),
    /// Run a single task non-interactively and capture the result (headless mode).
    Run(RunCliArgs),
    /// Read messages from an agent's inbox (`.bwoc/inbox.jsonl`).
    Inbox(InboxArgs),
    /// Tail an agent's daemon log (`.bwoc/agent.log`) — daemon stderr.
    Log(LogArgs),
    /// Read workspace-level memory (`.bwoc/memory/`).
    #[command(subcommand)]
    Memory(MemoryAction),
    /// Manage Saṅgha teams — a named subset of agents sharing a task list.
    #[command(subcommand)]
    Team(TeamCommand),
    /// Manage a team's shared task list (add / list / claim / complete).
    #[command(subcommand)]
    Task(TaskCommand),
    /// View peer workspaces declared in routes.toml (read-only cross-workspace view, #20).
    #[command(subcommand)]
    Peer(PeerCommand),
    /// List running agent sessions detected via markers and process scan (#21).
    Sessions(SessionsArgs),
    /// Check for, or delegate, an upgrade of bwoc to the latest release.
    Update {
        /// Read-only: compare the binary's embedded CalVer to the latest GitHub
        /// release tag and report drift (no changes made).
        #[arg(long)]
        check: bool,
        /// When upgrading, execute the delegated command (e.g. `brew upgrade
        /// bwoc`) instead of only printing it. Raw binaries are never self-swapped.
        #[arg(long)]
        run: bool,
    },
    /// Manage workspace notes (YYYY-MM-DD_<slug>.md in notes/).
    #[command(subcommand)]
    Notes(DocSubcommand),
    /// Manage workspace retrospectives (YYYY-MM-DD_<slug>.md in retrospectives/).
    #[command(name = "retro", subcommand)]
    Retro(DocSubcommand),
    /// Manage workspace research documents (YYYY-MM-DD_<slug>.md in research/).
    #[command(subcommand)]
    Research(DocSubcommand),
    /// Manage documents of any kind — built-in or workspace-declared custom
    /// (via `.bwoc/doc-kinds.toml`). Use this for custom kinds; the named
    /// aliases (`notes`, `retro`, `research`) are thin wrappers over this.
    #[command(name = "doc", subcommand)]
    Doc(DocKindSubcommand),
    /// Fleet governance — Aparihāniya-dhamma 7 health signals (read-only, report-only).
    #[command(name = "fleet", subcommand)]
    Fleet(FleetCommand),
    /// Framework skills under `modules/skills/<name>/` — list, show, verify
    /// (read), plus init, install, enable, disable, remove (write).
    /// See `docs/en/SKILLS.en.md`.
    #[command(name = "skill", subcommand)]
    Skill(SkillCommand),
    /// Framework plugins under `modules/plugins/<name>/` — list, show
    /// (read), plus init, install, enable, disable, remove (write).
    /// No `verify` in v1 (PLUGINS.en.md §"CLI Surface" line 314).
    /// See `docs/en/PLUGINS.en.md`.
    #[command(name = "plugin", subcommand)]
    Plugin(PluginCommand),
    /// Run audit-kind framework plugins and emit a canonical findings report.
    /// Discovers `modules/plugins/<name>/manifest.toml` with `kind = "audit"`;
    /// invokes each enabled plugin's `[plugin].entry`; validates findings
    /// against the BWOC-11 normative schema (PLUGINS.en.md §"Audit Findings
    /// Schema"). Exit code = `fail` finding count (clamped to 254); `255`
    /// signals a framework/plugin error.
    #[command(name = "audit", subcommand)]
    Audit(AuditCommand),
    /// Operate the `jira` plugin kind: sync the scrum backlog with Jira, run
    /// project-scoped JQL, transition issues, and link/unlink story ↔ issue
    /// mappings in `.scrum/jira-sync.json`. Reads + writes an external tracker,
    /// so write verbs are gated behind confirmation. Credentials come from
    /// `BWOC_JIRA_{EMAIL,TOKEN,BASE_URL}` env (never committed); a missing one
    /// exits 2. The live REST path is provided by an enabled `jira`-kind plugin
    /// (see PLUGINS.en.md §jira); without one, live verbs stub-error (exit 4).
    /// Every verb has a `--json` twin. See `docs/en/PLUGINS.en.md`.
    #[command(name = "jira", subcommand)]
    Jira(jira::JiraCommand),

    /// Expose a local agent over the A2A (Agent2Agent) protocol — print its
    /// Agent Card or run the JSON-RPC HTTP listener (loopback-only by default;
    /// no auth yet). See #48.
    #[command(name = "a2a", subcommand)]
    A2a(A2aCommand),
}

#[derive(clap::Subcommand, Debug)]
enum A2aCommand {
    /// Print the agent's A2A Agent Card (JSON) derived from its manifest.
    Card(A2aCardArgs),
    /// Run the A2A HTTP listener: Agent Card at the well-known path + a
    /// JSON-RPC endpoint that drops inbound messages into the agent's inbox.
    Serve(A2aServeArgs),
    /// Fetch and print an external agent's A2A Agent Card (discovery).
    FetchCard(A2aFetchCardArgs),
    /// Send a text message to an external A2A agent via SendMessage.
    Send(A2aSendArgs),
}

#[derive(Args, Debug)]
struct A2aFetchCardArgs {
    /// Base URL of the remote A2A agent (the well-known path is appended).
    url: String,
}

#[derive(Args, Debug)]
struct A2aSendArgs {
    /// The remote agent's JSON-RPC endpoint URL.
    url: String,
    /// Message text to send.
    message: String,
    /// Optional A2A `contextId` to associate the message with.
    #[arg(long)]
    context: Option<String>,
}

#[derive(Args, Debug)]
struct A2aCardArgs {
    /// Agent name or id (the `agent-` prefix is optional).
    agent: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct A2aServeArgs {
    /// Agent name or id (the `agent-` prefix is optional).
    agent: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Address to bind. Defaults to loopback (`127.0.0.1`); a non-loopback
    /// value warns, since the listener has no authentication yet.
    #[arg(long, default_value = "127.0.0.1")]
    bind: std::net::IpAddr,
    /// TCP port to listen on.
    #[arg(long, default_value_t = 41241)]
    port: u16,
    /// Expose this team's shared task list over A2A `tasks/*` (GetTask/ListTasks).
    #[arg(long)]
    team: Option<String>,
}

#[derive(clap::Subcommand, Debug)]
enum AuditCommand {
    /// Run audit-kind plugins. Default scope: every plugin enabled in
    /// `workspace.toml [plugins.<name>]` with `kind = "audit"`.
    Run(AuditRunArgs),
}

#[derive(Args, Debug)]
struct AuditRunArgs {
    /// Scope to one audit plugin (must match a directory name under
    /// `modules/plugins/` whose manifest has `kind = "audit"`). Overrides
    /// the default "all enabled" set; runs regardless of `enabled` flag.
    #[arg(long)]
    plugin: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope
    /// `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }], summary }`
    /// instead of the human-readable table. Findings serialize in plugin-emit
    /// order (criterion-declaration order per BWOC-11 line 84).
    #[arg(long)]
    json: bool,
}

#[derive(clap::Subcommand, Debug)]
enum PluginCommand {
    /// List installed framework plugins (`modules/plugins/<name>/manifest.toml`).
    List(PluginListArgs),
    /// Show one plugin's manifest + SPEC location + workspace registration.
    Show(PluginShowArgs),
    /// Scaffold a new framework plugin from `modules/plugin-template/`.
    Init(PluginInitArgs),
    /// Install a plugin from local path / git URL / tarball URL (SHA-256 trust gate).
    Install(PluginInstallArgs),
    /// Enable a plugin in workspace.toml (sets `[plugins.<name>] enabled = true`).
    Enable(PluginEnableArgs),
    /// Disable a plugin in workspace.toml (keeps the entry; sets `enabled = false`).
    Disable(PluginDisableArgs),
    /// Delete `modules/plugins/<name>/` and remove the `[plugins.<name>]` table.
    Remove(PluginRemoveArgs),
}

#[derive(Args, Debug)]
struct PluginListArgs {
    /// Filter to plugins enabled in `workspace.toml [plugins.<name>]`.
    #[arg(long)]
    enabled: bool,
    /// Filter to one plugin kind. Valid values today:
    /// `audit`, `memory-backend`, `llm-backend`, `workflow`.
    #[arg(long)]
    kind: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON `{ workspace, plugins: [...] }` instead of the human table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PluginShowArgs {
    /// Plugin name — must match a directory under `modules/plugins/`.
    name: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON `{ workspace, plugin: { ... } }` instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PluginInitArgs {
    /// Plugin name — kebab-case; becomes the directory name under `modules/plugins/`.
    name: String,
    /// Plugin kind — one of: `memory-backend`, `llm-backend`, `workflow`, `audit`.
    /// Required; no default (PLUGINS.en.md §"Scaffolding from template" line 398).
    #[arg(long)]
    kind: String,
    /// Override `{{pluginVersion}}`. Default `0.1.0`.
    #[arg(long)]
    version: Option<String>,
    /// Override `{{pluginDescription}}`. Default a hint placeholder.
    #[arg(long)]
    description: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PluginInstallArgs {
    /// Source — local path (`./`, `../`, `/`), git URL (`*.git[#ref]`), or tarball URL (`*.tar.gz` / `*.tgz`).
    source: String,
    /// Skip the SHA-256 trust gate. Emits a stderr warning.
    #[arg(long = "no-verify")]
    no_verify: bool,
    /// Required the first time a given source URL is installed in this workspace.
    #[arg(long = "allow-new-source")]
    allow_new_source: bool,
    /// Replace an existing install in place.
    #[arg(long)]
    upgrade: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PluginEnableArgs {
    /// Plugin name (must be installed under `modules/plugins/`).
    name: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PluginDisableArgs {
    /// Plugin name (must already have a `[plugins.<name>]` entry in workspace.toml).
    name: String,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PluginRemoveArgs {
    /// Plugin name to remove (deletes `modules/plugins/<name>/`).
    name: String,
    /// Skip the confirmation prompt. Required with `--json`.
    #[arg(long)]
    yes: bool,
    /// Also drop the matching entry in `.bwoc/installed-sources.toml`.
    #[arg(long = "forget-source")]
    forget_source: bool,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(clap::Subcommand, Debug)]
enum SkillCommand {
    /// List installed framework skills (`modules/skills/<name>/manifest.toml`).
    List(SkillListArgs),
    /// Show one skill's manifest + SPEC location (full detail).
    Show(SkillShowArgs),
    /// Statically check skills and print each `[gates].verify` command without
    /// running it. Pass --run-gates to execute them — they come from an
    /// UNTRUSTED manifest. Exits non-zero on any failure.
    Verify(SkillVerifyArgs),
    /// Scaffold a new framework skill from `modules/skill-template/`.
    Init(SkillInitArgs),
    /// Install a skill from local path / git URL / tarball URL (SHA-256 trust gate).
    Install(SkillInstallArgs),
    /// Enable a skill on the current agent (sets `enabled = true`).
    Enable(SkillEnableArgs),
    /// Disable a skill on the current agent (keeps the entry; sets `enabled = false`).
    Disable(SkillDisableArgs),
    /// Delete `modules/skills/<name>/` and clean every consuming agent's manifest.
    Remove(SkillRemoveArgs),
}

#[derive(Args, Debug)]
struct SkillInitArgs {
    /// Skill name — kebab-case; becomes the directory name under `modules/skills/`.
    name: String,
    /// Override `{{skillVersion}}`. Default `0.1.0`.
    #[arg(long)]
    version: Option<String>,
    /// Override `{{skillDescription}}`. Default a hint placeholder.
    #[arg(long)]
    description: Option<String>,
    /// Override `{{skillOperation}}`. Default `<name>_op` (snake-cased).
    #[arg(long)]
    operation: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillInstallArgs {
    /// Source — local path (`./`, `../`, `/`), git URL (`*.git[#ref]`), or tarball URL (`*.tar.gz` / `*.tgz`).
    source: String,
    /// Skip the SHA-256 trust gate. Emits a stderr warning.
    #[arg(long = "no-verify")]
    no_verify: bool,
    /// Required the first time a given source URL is installed in this workspace.
    #[arg(long = "allow-new-source")]
    allow_new_source: bool,
    /// Replace an existing install in place.
    #[arg(long)]
    upgrade: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillEnableArgs {
    /// Skill name (must be installed under `modules/skills/`).
    name: String,
    /// Override the current-agent resolution (default: cwd descent / BWOC_AGENT).
    #[arg(long)]
    agent: Option<String>,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillDisableArgs {
    /// Skill name (must already have a `skills.framework[]` entry on this agent).
    name: String,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillRemoveArgs {
    /// Skill name to remove (deletes `modules/skills/<name>/`).
    name: String,
    /// Skip the confirmation prompt. Required with `--json`.
    #[arg(long)]
    yes: bool,
    /// Also drop the matching entry in `.bwoc/installed-sources.toml`.
    #[arg(long = "forget-source")]
    forget_source: bool,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillListArgs {
    /// Filter to skills enabled on the current agent (resolved from
    /// `--agent`, `BWOC_AGENT` env, or cwd → `<workspace>/agents/<id>/`).
    #[arg(long)]
    enabled: bool,
    /// Override the current-agent resolution (pair with `--enabled` to
    /// inspect what is enabled on a specific agent without changing cwd).
    #[arg(long)]
    agent: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON `{ workspace, skills: [...] }` instead of the human table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillShowArgs {
    /// Skill name — must match a directory under `modules/skills/`.
    name: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON `{ workspace, skill: { ... } }` instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["name", "all"])))]
struct SkillVerifyArgs {
    /// Skill name. Mutually exclusive with `--all`.
    name: Option<String>,
    /// Verify every installed skill. Mutually exclusive with `name`.
    #[arg(long)]
    all: bool,
    /// Execute each `[gates].verify` command via `sh -c`. SECURITY: these
    /// commands come from the skill manifest (UNTRUSTED input) — off by
    /// default. Without this flag, verify only prints the commands it would run.
    #[arg(long = "run-gates")]
    run_gates: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON `{ workspace, ok, gates_executed, results: [...] }` instead of the human report.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Subcommand, Debug)]
enum FleetCommand {
    /// Check all 7 Aparihāniya-dhamma fleet-governance signals (read-only).
    Health(FleetHealthArgs),
}

#[derive(Args, Debug)]
struct FleetHealthArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON array instead of the human report.
    #[arg(long)]
    json: bool,
    /// Days without activity before an agent is considered stale (condition 1).
    #[arg(long = "stale-days", default_value_t = 7)]
    stale_days: u64,
}

#[derive(clap::Subcommand, Debug)]
enum TeamCommand {
    /// Create a team with a member list (`--members a,b,c`).
    Create {
        /// Team id (kebab-case by convention).
        id: String,
        /// Comma-separated agent ids that belong to the team.
        #[arg(long, value_delimiter = ',')]
        members: Vec<String>,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Emit JSON instead of the human report.
        #[arg(long)]
        json: bool,
    },
    /// List teams in the workspace with member + task counts.
    List {
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Retire a team — remove its membership file + task list.
    Retire {
        /// Team id to retire.
        id: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Subcommand, Debug)]
enum TaskCommand {
    /// Add a task to a team's list. Optional `--deps a,b` gate it on others.
    Add {
        /// Team id the task belongs to.
        team: String,
        /// Human-readable task title.
        title: String,
        /// Comma-separated task ids that must complete before this is claimable.
        #[arg(long, value_delimiter = ',')]
        deps: Vec<String>,
        /// Explicit task id (default: auto `t<N>`).
        #[arg(long = "id")]
        id: Option<String>,
        /// Gate completion on lead plan approval (Pavāraṇā): the claimant must
        /// submit a plan and the lead must approve it before `task complete`.
        #[arg(long = "requires-plan")]
        requires_plan: bool,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// List a team's tasks with state + claimant.
    List {
        /// Team id.
        team: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Claim a pending, unblocked task as an agent (`--as <agent>`).
    Claim {
        /// Team id.
        team: String,
        /// Task id to claim.
        task: String,
        /// Claiming agent id (must be a team member).
        #[arg(long = "as")]
        agent: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Complete an in-progress task you claimed (`--as <agent>`).
    Complete {
        /// Team id.
        team: String,
        /// Task id to complete.
        task: String,
        /// Completing agent id (must be the claimant).
        #[arg(long = "as")]
        agent: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Submit/revise a plan for a claimed task, or show the current plan (Pavāraṇā).
    Plan {
        /// Team id.
        team: String,
        /// Task id.
        task: String,
        /// Submitting agent id (the claimant). Required to submit; omit to just show.
        #[arg(long = "as")]
        agent: Option<String>,
        /// Plan text. Mutually exclusive with --plan-file. Omit both to show the plan.
        #[arg(long, conflicts_with = "plan_file")]
        plan: Option<String>,
        /// Read the plan body from a file. Mutually exclusive with --plan.
        #[arg(long = "plan-file")]
        plan_file: Option<PathBuf>,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Lead approves a submitted plan — the claimant may then complete the task.
    Approve {
        /// Team id.
        team: String,
        /// Task id.
        task: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Lead rejects a submitted plan — the claimant must revise + resubmit.
    Reject {
        /// Team id.
        team: String,
        /// Task id.
        task: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Subcommand, Debug)]
enum MemoryAction {
    /// List user-authored memory entries in `.bwoc/memory/`.
    List {
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Emit JSON instead of the human table.
        #[arg(long)]
        json: bool,
        /// Print just the entry count (integer or `{"count": N}` with --json).
        #[arg(long)]
        count: bool,
        /// Print bare entry filenames, one per line (or `{"names": [...]}` with --json).
        #[arg(long = "names-only")]
        names_only: bool,
        /// Sort key. Default: name (alphabetical). One of: name, size, modified.
        #[arg(long)]
        sort: Option<String>,
    },
    /// Print one memory entry's contents to stdout, or `--all` for every entry concatenated.
    Show {
        /// Entry name (with or without `.md` extension). Omit when using `--all`.
        name: Option<String>,
        /// Print every entry concatenated (alphabetical), each with a `# === <name> ===` header.
        /// Mutually exclusive with `<name>`.
        #[arg(long, conflicts_with = "name")]
        all: bool,
        /// Workspace root. Resolution chain same as `memory list`.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Emit JSON (only meaningful with `--all`; single-entry `show` is plain content).
        #[arg(long)]
        json: bool,
    },
    /// Write a memory entry. Source precedence: inline `[content]` > `--file` > stdin.
    Put {
        /// Entry name (with or without `.md` extension).
        name: String,
        /// Inline content. If given, used as the entry body verbatim
        /// (with a trailing newline appended if missing). Skips both
        /// `--file` and stdin.
        content: Option<String>,
        /// Source file. Used when `[content]` is omitted.
        #[arg(long, conflicts_with = "content")]
        file: Option<PathBuf>,
        /// Overwrite an existing entry. Mutually exclusive with `--append`.
        #[arg(long, conflicts_with = "append")]
        force: bool,
        /// Append to an existing entry (newline-separated). Mutually exclusive with `--force`.
        #[arg(long)]
        append: bool,
        /// Workspace root. Resolution chain same as `memory list`.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// Substring search across memory entries (case-insensitive).
    Search {
        /// Substring to look for in any entry's content.
        query: String,
        /// Workspace root. Resolution chain same as `memory list`.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Emit JSON instead of the human-readable grep-style output.
        #[arg(long)]
        json: bool,
    },
    /// Delete a memory entry. Prompts on TTY unless `--yes` is given.
    Rm {
        /// Entry name (with or without `.md` extension).
        name: String,
        /// Skip the TTY confirmation prompt.
        #[arg(long, short)]
        yes: bool,
        /// Workspace root. Resolution chain same as `memory list`.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    // ------------------------------------------------------------------
    // Tier 2 — deep-memory backend (optional; non-fatal when not configured)
    // ------------------------------------------------------------------
    /// Tier 2: emit prior context at session start (`deepMemoryCmd wake-up`).
    WakeUp {
        /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
        agent: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// Tier 2: search past decisions/notes (`deepMemoryCmd search "<query>"`).
    /// Named `t2-search` to avoid collision with the existing Tier 1 `search` subcommand.
    #[command(name = "t2-search")]
    T2Search {
        /// Search query string.
        query: String,
        /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
        agent: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// Tier 2: persist session learnings at session end (`deepMemoryCmd mine <path> --mode <mode>`).
    Mine {
        /// Path to the agent's sessions directory (passed verbatim to the backend).
        path: PathBuf,
        /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
        agent: String,
        /// Mining mode — tool-defined string (e.g. `convos`). Default: `convos`.
        #[arg(long, default_value = "convos")]
        mode: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
}

/// Sub-actions shared by all document-kind commands (`notes`, `retro`, `research`).
#[derive(clap::Subcommand, Debug)]
enum DocSubcommand {
    /// Create a new document with the given title.
    New {
        /// Document title (used as the filename slug).
        title: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// List documents of this kind (newest first).
    List {
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// Print a document matching a date prefix or exact filename.
    View {
        /// Date prefix (e.g. `2026-05-24`) or full filename stem.
        name: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
}

/// Generic `bwoc doc <kind> <action>` subcommand — resolves built-in OR
/// workspace-declared custom kinds from `.bwoc/doc-kinds.toml`.
#[derive(clap::Subcommand, Debug)]
enum DocKindSubcommand {
    /// Create a new document of the given kind with the given title.
    New {
        /// Document kind name (built-in: notes, retrospectives, research;
        /// or any name declared in `.bwoc/doc-kinds.toml`).
        kind: String,
        /// Document title (used as the filename slug).
        title: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// List documents of the given kind (newest first).
    List {
        /// Document kind name.
        kind: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// Print a document of the given kind matching a date prefix or exact filename.
    View {
        /// Document kind name.
        kind: String,
        /// Date prefix (e.g. `2026-05-24`) or full filename stem.
        name: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
}

/// `bwoc peer` subcommands — read-only cross-workspace view (#20).
///
/// Give-feedback (write) is deferred to a later phase.
#[derive(clap::Subcommand, Debug)]
enum PeerCommand {
    /// List peers declared in this workspace's routes.toml.
    List {
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// View a peer's agents and open team tasks.
    /// Alias: `bwoc peer <key>` (no subcommand) is not yet supported in clap;
    /// use `bwoc peer status <key>` or `bwoc peer view <key>`.
    #[command(name = "status")]
    View {
        /// Peer key as declared in routes.toml (agent id or namespace prefix).
        key: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// List or view shared documents from a peer's allowlist
    /// (`<peer>/.bwoc/interconnect/shared.toml`).
    ///
    /// Without `<doc>`: list all shared documents.
    /// With `<doc>`: print the contents of one named document (allowlist-gated).
    Learn {
        /// Peer key as declared in routes.toml.
        key: String,
        /// Optional document name (filename with or without `.md`).
        /// Omit to list all shared documents.
        doc: Option<String>,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
    /// Give a peer agent signed cross-workspace feedback (#20). The review is
    /// delivered as a signed envelope (`kind: feedback`) into the peer agent's
    /// inbox; the recipient verifies the sender's signature before accepting.
    Feedback {
        /// Recipient peer agent id (resolved via routes.toml), e.g. "agent-oracle".
        agent: String,
        /// The feedback / review text.
        message: String,
        /// Sender agent (required — feedback must be signed by a local agent).
        #[arg(long = "from")]
        from: String,
        /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
    },
}

impl MemoryAction {
    fn into_runtime(self) -> memory::MemoryArgs {
        match self {
            MemoryAction::List {
                workspace,
                json,
                count,
                names_only,
                sort,
            } => memory::MemoryArgs {
                action: memory::MemoryAction::List,
                workspace,
                json,
                count_only: count,
                names_only,
                sort,
            },
            MemoryAction::Show {
                name,
                all,
                workspace,
                json,
            } => {
                // If neither `<name>` nor `--all` was provided, pass an
                // empty Show("") through. memory::show() detects this and
                // emits a helpful error. This keeps into_runtime
                // infallible (returns MemoryArgs, not Result).
                let action = if all {
                    memory::MemoryAction::ShowAll
                } else {
                    memory::MemoryAction::Show(name.unwrap_or_default())
                };
                memory::MemoryArgs {
                    action,
                    workspace,
                    json,
                    count_only: false,
                    names_only: false,
                    sort: None,
                }
            }
            MemoryAction::Put {
                name,
                content,
                file,
                force,
                append,
                workspace,
            } => {
                // Precedence: inline content > --file > stdin. clap
                // already enforces (content, file) mutex AND (force, append) mutex.
                let source = match (content, file) {
                    (Some(c), _) => memory::PutSource::Inline(c),
                    (None, Some(p)) => memory::PutSource::FilePath(p),
                    (None, None) => memory::PutSource::Stdin,
                };
                memory::MemoryArgs {
                    action: memory::MemoryAction::Put {
                        name,
                        source,
                        force,
                        append,
                    },
                    workspace,
                    json: false,
                    count_only: false,
                    names_only: false,
                    sort: None,
                }
            }
            MemoryAction::Search {
                query,
                workspace,
                json,
            } => memory::MemoryArgs {
                action: memory::MemoryAction::Search(query),
                workspace,
                json,
                count_only: false,
                names_only: false,
                sort: None,
            },
            MemoryAction::Rm {
                name,
                yes,
                workspace,
            } => memory::MemoryArgs {
                action: memory::MemoryAction::Remove { name, yes },
                workspace,
                json: false,
                count_only: false,
                names_only: false,
                sort: None,
            },
            // Tier 2 variants are routed directly to deep_memory_cmd in the
            // main() dispatch and will never reach into_runtime. The
            // unreachable! here keeps the exhaustiveness check satisfied.
            MemoryAction::WakeUp { .. }
            | MemoryAction::T2Search { .. }
            | MemoryAction::Mine { .. } => {
                unreachable!("Tier 2 variants are dispatched before into_runtime is called")
            }
        }
    }
}

#[derive(Args, Debug)]
struct LogArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
    agent: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Block + stream new lines as they arrive (Ctrl-C to stop).
    #[arg(short = 'f', long)]
    follow: bool,
    /// Number of trailing lines to print before --follow blocks (or as the whole output).
    #[arg(short = 'n', long, default_value_t = 50)]
    lines: usize,
    /// Truncate the log file before printing. Useful when starting fresh observation.
    #[arg(long)]
    clear: bool,
}

impl From<LogArgs> for log::LogArgs {
    fn from(a: LogArgs) -> Self {
        Self {
            agent: a.agent,
            workspace: a.workspace,
            follow: a.follow,
            lines: a.lines,
            clear: a.clear,
        }
    }
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["agent", "all"])))]
struct InboxArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
    /// Mutually exclusive with `--all`.
    agent: Option<String>,
    /// Print every agent's inbox concatenated (each with a header).
    /// Mutually exclusive with `<agent>`. `--clear` and `--watch` are
    /// refused with `--all`.
    #[arg(long)]
    all: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human-readable layout.
    #[arg(long)]
    json: bool,
    /// Show only the last N messages.
    #[arg(long)]
    limit: Option<usize>,
    /// Tail mode — block and print new envelopes as they arrive (Ctrl-C to stop).
    #[arg(long)]
    watch: bool,
    /// Truncate the inbox after printing (acknowledge / delete all messages).
    #[arg(long)]
    clear: bool,
    /// Skip the interactive confirmation for `--clear`. Required for non-TTY.
    #[arg(long)]
    yes: bool,
    /// Print just the envelope count (one integer) instead of messages.
    /// With `--json`, emits `{"count": N}`.
    #[arg(long)]
    count: bool,
}

impl From<InboxArgs> for inbox::InboxArgs {
    fn from(a: InboxArgs) -> Self {
        Self {
            agent: a.agent.unwrap_or_default(), // clap group ensures one of (agent, all)
            workspace: a.workspace,
            json: a.json,
            limit: a.limit,
            watch: a.watch,
            clear: a.clear,
            yes: a.yes,
            count: a.count,
            all: a.all,
        }
    }
}

#[derive(Args, Debug)]
struct ChatArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
    name: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Run inside a new tmux window instead of exec'ing in this shell. Requires $TMUX.
    #[arg(long, conflicts_with = "ghostty")]
    tmux: bool,
    /// Open a new Ghostty terminal window instead of exec'ing in this shell. macOS-only.
    #[arg(long)]
    ghostty: bool,
}

impl ChatArgs {
    fn into_runtime(self, lang: String) -> chat::ChatArgs {
        chat::ChatArgs {
            name: self.name,
            workspace: self.workspace,
            lang,
            tmux: self.tmux,
            ghostty: self.ghostty,
        }
    }
}

#[derive(Args, Debug)]
struct RunCliArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
    agent: String,
    /// Task prompt to deliver to the agent (required).
    #[arg(long)]
    task: String,
    /// Emit structured JSON `{ agent, backend, task, exit_code, duration_ms, output }` to stdout.
    #[arg(long)]
    json: bool,
    /// Kill the agent process and report timeout if it runs longer than this many seconds.
    #[arg(long)]
    timeout: Option<u64>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

impl From<RunCliArgs> for run::RunArgs {
    fn from(a: RunCliArgs) -> Self {
        Self {
            agent: a.agent,
            task: a.task,
            json: a.json,
            timeout_secs: a.timeout,
            workspace: a.workspace,
        }
    }
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("body").required(true).args(["message", "file"])))]
// clap renders the required `body` group ahead of the plain `to` positional,
// producing a usage line (`<MESSAGE|--file> <TO>`) that contradicts the actual
// parse order (`to` is positional 1, `message` is positional 2). Pin the usage
// string to reality so the help text doesn't mislead. (Yoniso Manasikāra —
// the docs match the behaviour, not clap's default rendering.)
#[command(override_usage = "bwoc send [OPTIONS] <TO> <MESSAGE|--file <FILE>>")]
struct SendArgs {
    /// Recipient agent. Matches by id ("agent-foo") or bare name ("foo").
    to: String,
    /// Message text (quote multi-word). Mutually exclusive with `--file`.
    message: Option<String>,
    /// Source file. The file's full contents (minus trailing newlines) becomes
    /// the message body. Mutually exclusive with `<message>`.
    #[arg(long)]
    file: Option<PathBuf>,
    /// Sender identity. Default is "user" (human operator). Pass an agent
    /// name ("foo" or "agent-foo") for agent → agent messaging — the
    /// named sender must exist in the workspace registry. The recipient's
    /// trust gate (when on) evaluates against this sender's manifest.
    /// See modules/agent-template/interconnect/messaging.md.
    #[arg(long = "from")]
    from: Option<String>,
    /// Thread this send as a reply to a prior envelope. Value is the prior
    /// envelope's `messageId` ("msg-<slug>-<hex>"). Stamped as `replyTo`
    /// in the new envelope; the auto-reply Stop hook uses this to close
    /// a request/response loop.
    #[arg(long = "reply-to")]
    reply_to: Option<String>,
    /// Skip the best-effort tmux send-keys wakeup ping. CI, daemons, and
    /// the auto-reply hook pass this to avoid side-effecting into a TUI
    /// session that didn't ask for an interruption.
    #[arg(long = "no-wakeup")]
    no_wakeup: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

impl SendArgs {
    /// Resolve the message body — file content or inline arg. clap's
    /// ArgGroup already guarantees exactly one is set.
    fn resolve_message(self) -> Result<send::SendArgs, String> {
        let body = match (self.message, self.file) {
            (Some(m), _) => m,
            (None, Some(p)) => std::fs::read_to_string(&p)
                .map_err(|e| format!("failed to read {}: {e}", p.display()))?
                // Trim trailing newlines (a vim/EOF newline shouldn't bloat
                // the JSONL envelope; explicit content stays preserved).
                .trim_end_matches('\n')
                .to_string(),
            (None, None) => unreachable!("clap ArgGroup enforces one of (message, file)"),
        };
        Ok(send::SendArgs {
            to: self.to,
            message: body,
            from: self.from,
            reply_to: self.reply_to,
            no_wakeup: self.no_wakeup,
            workspace: self.workspace,
            kind: None,
            force_peer_route: false,
            require_signed: false,
        })
    }
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["name", "all"])))]
struct PingArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
    /// Mutually exclusive with `--all` (clap group enforces).
    name: Option<String>,
    /// Ping every agent in the workspace. Agents with no live socket
    /// (not running) are labeled but don't fail the run.
    #[arg(long)]
    all: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

impl From<PingArgs> for ping::PingArgs {
    fn from(a: PingArgs) -> Self {
        Self {
            name: a.name.unwrap_or_default(), // clap group ensures one of (name, all)
            workspace: a.workspace,
            all: a.all,
        }
    }
}

#[derive(Args, Debug)]
struct SuperviseArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo").
    agent: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Max restarts within a rolling 60s window. Default 10. Beyond this,
    /// the supervisor gives up rather than burn CPU on a crash loop.
    #[arg(long = "max-restarts-per-min", default_value_t = 10)]
    max_restarts_per_min: usize,
    /// Emit one JSON event per action (spawn / crash_respawn / clean_exit /
    /// rate_limit_hit / signal_stop) to stdout. For monitoring pipelines.
    #[arg(long)]
    json: bool,
}

impl From<SuperviseArgs> for supervise::SuperviseArgs {
    fn from(a: SuperviseArgs) -> Self {
        Self {
            agent: a.agent,
            workspace: a.workspace,
            max_restarts_per_min: a.max_restarts_per_min,
            json: a.json,
        }
    }
}

#[derive(Args, Debug)]
struct TrustArgs {
    /// Agent name ("agent-foo" or "foo"). Optional only with `--keygen --all`.
    agent: Option<String>,
    /// Generate an ed25519 signing keypair (HV2-4) instead of reading the
    /// profile: private key → <agent>/.bwoc/agent.key (0600), public key →
    /// manifest `signingPublicKey`.
    #[arg(long)]
    keygen: bool,
    /// With `--keygen`: generate for every registered agent (backfill).
    #[arg(long)]
    all: bool,
    /// With `--keygen`: overwrite an existing key (rotates the agent's identity).
    #[arg(long)]
    force: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

impl From<TrustArgs> for trust::TrustArgs {
    fn from(a: TrustArgs) -> Self {
        Self {
            agent: a.agent,
            workspace: a.workspace,
            json: a.json,
            keygen: a.keygen,
            all: a.all,
            force: a.force,
        }
    }
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["name", "all"])))]
struct StartArgs {
    /// Name of the agent. Matches by id ("agent-foo") or bare name ("foo").
    /// Mutually exclusive with `--all` (clap group enforces).
    name: Option<String>,
    /// Start every stopped agent in the workspace. Honors `--yes` + `--no-daemon`.
    /// Mutually exclusive with `name`.
    #[arg(long)]
    all: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Skip the interactive confirmation. Required for non-TTY (scripted) use.
    #[arg(long)]
    yes: bool,
    /// Only flip registry status; do not spawn `bwoc-agent --serve`.
    #[arg(long = "no-daemon")]
    no_daemon: bool,
    /// Emit JSON `{ workspace, agent, daemon_spawned, daemon_pid, ... }`
    /// instead of the human report. Requires `--yes`. Single-agent only.
    #[arg(long)]
    json: bool,
}

impl From<StartArgs> for start::StartArgs {
    fn from(a: StartArgs) -> Self {
        Self {
            name: a.name.unwrap_or_default(), // clap group ensures one of (name, all)
            workspace: a.workspace,
            yes: a.yes,
            no_daemon: a.no_daemon,
            all: a.all,
            json: a.json,
        }
    }
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["name", "all"])))]
struct StopArgs {
    /// Name of the agent. Matches by id ("agent-foo") or bare name ("foo").
    /// Mutually exclusive with `--all` (clap group enforces).
    name: Option<String>,
    /// Stop every non-stopped agent in the workspace. Honors `--yes`.
    /// Mutually exclusive with `name`.
    #[arg(long)]
    all: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Skip the interactive confirmation. Required for non-TTY (scripted) use.
    #[arg(long)]
    yes: bool,
    /// Emit JSON `{ workspace, agent, daemon_outcome, registry_updated }`
    /// instead of the human report. Requires `--yes`. Single-agent only.
    #[arg(long)]
    json: bool,
}

impl From<StopArgs> for stop::StopArgs {
    fn from(a: StopArgs) -> Self {
        Self {
            name: a.name.unwrap_or_default(), // clap group ensures one of (name, all)
            workspace: a.workspace,
            yes: a.yes,
            all: a.all,
            json: a.json,
        }
    }
}

#[derive(Args, Debug)]
struct DashboardArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

impl DashboardArgs {
    fn into_runtime(self, lang: String) -> dashboard::DashboardArgs {
        dashboard::DashboardArgs {
            workspace: self.workspace,
            lang,
        }
    }
}

#[derive(Args, Debug)]
struct CompletionArgs {
    /// Target shell. Pipe the output to your shell's completion install path.
    #[arg(value_enum)]
    shell: clap_complete::Shell,
}

impl From<CompletionArgs> for completion::CompletionArgs {
    fn from(a: CompletionArgs) -> Self {
        Self { shell: a.shell }
    }
}

#[derive(Args, Debug)]
struct HelpArgs {
    /// Topic name. Run `bwoc help` (no arg) to list available topics.
    /// Mutually exclusive with `--all`.
    #[arg(conflicts_with = "all")]
    topic: Option<String>,
    /// Print every topic concatenated (with `# === <name> ===` separators).
    /// Useful for offline reading or piping into a docs generator.
    #[arg(long)]
    all: bool,
}

impl From<HelpArgs> for help::HelpArgs {
    fn from(a: HelpArgs) -> Self {
        Self {
            topic: a.topic,
            all: a.all,
        }
    }
}

#[derive(Args, Debug)]
struct StatusArgs {
    /// Agent name. Matches by id ("agent-foo") or bare name ("foo"). If omitted, shows the
    /// per-agent table summary. Mutually exclusive with `--all`.
    #[arg(conflicts_with = "all")]
    name: Option<String>,
    /// Print the full detail block for every agent (loops the single-agent view).
    /// Mutually exclusive with `name` and `--banner`.
    #[arg(long, conflicts_with = "banner")]
    all: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON to stdout instead of the human-readable layout.
    #[arg(long)]
    json: bool,
    /// Replay the agent's startup liveness banner from its manifest (read-only, no daemon required).
    /// Requires a named agent. Mutually exclusive with `--all`.
    #[arg(long, requires = "name", conflicts_with = "all")]
    banner: bool,
}

impl From<StatusArgs> for status::StatusArgs {
    fn from(a: StatusArgs) -> Self {
        Self {
            name: a.name,
            workspace: a.workspace,
            json: a.json,
            all: a.all,
            banner: a.banner,
            lang: String::new(), // overwritten by main() before run() is called
        }
    }
}

#[derive(Args, Debug)]
struct RetireArgs {
    /// Name of the agent to retire. Matches by id ("agent-foo") or bare name ("foo").
    name: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Skip the interactive confirmation. Required for non-TTY (scripted) use.
    #[arg(long)]
    yes: bool,
    /// Keep the agent directory on disk; only remove the registry entry.
    /// Mutually exclusive with `--keep-memory`.
    #[arg(long = "keep-files", conflicts_with = "keep_memory")]
    keep_files: bool,
    /// Preserve just `memories/` while removing the rest of the agent dir.
    /// Lets users retire an agent but keep its accumulated knowledge.
    /// Mutually exclusive with `--keep-files`.
    #[arg(long = "keep-memory")]
    keep_memory: bool,
    /// Emit JSON `{ workspace, agent, path, mode, registry_updated }`
    /// instead of the human report. Requires `--yes` (scripted destructive
    /// ops without explicit ack → exit 2).
    #[arg(long)]
    json: bool,
}

impl From<RetireArgs> for retire::RetireArgs {
    fn from(a: RetireArgs) -> Self {
        Self {
            name: a.name,
            workspace: a.workspace,
            yes: a.yes,
            keep_files: a.keep_files,
            keep_memory: a.keep_memory,
            json: a.json,
        }
    }
}

#[derive(Args, Debug)]
struct DoctorArgs {
    /// Workspace root to diagnose. Defaults: BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    path: Option<PathBuf>,
    /// Attempt to fix safe issues automatically (missing dirs, missing symlinks).
    #[arg(long)]
    auto: bool,
    /// Emit structured JSON instead of the human-readable list.
    #[arg(long)]
    json: bool,
}

impl From<DoctorArgs> for doctor::DoctorArgs {
    fn from(a: DoctorArgs) -> Self {
        Self {
            path: a.path,
            auto: a.auto,
            json: a.json,
        }
    }
}

#[derive(Subcommand, Debug)]
enum WorkspaceCommand {
    /// Show resolved workspace path, config, and agent count.
    Info {
        /// Workspace root path. Defaults to current directory.
        path: Option<PathBuf>,
        /// Emit JSON instead of the human-readable layout.
        #[arg(long)]
        json: bool,
        /// Print just the resolved workspace path (for `cd "$(bwoc workspace info --path-only)"`).
        #[arg(long = "path-only")]
        path_only: bool,
    },
    /// Run validation rules; exit 0 if complete, 2 if violations.
    Validate {
        /// Workspace root path. Defaults to current directory.
        path: Option<PathBuf>,
        /// Emit JSON instead of the human-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Find inconsistencies (phantom registry entries, orphan dirs); --apply to fix safe ones.
    Prune {
        /// Workspace root path. Defaults to current directory.
        path: Option<PathBuf>,
        /// Apply the safe removals (phantom entries). Orphan dirs are never auto-removed.
        #[arg(long)]
        apply: bool,
        /// Emit JSON `{ workspace, phantoms, orphans, applied, removed }` for CI gating.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args, Debug)]
struct ListArgs {
    /// Workspace root path. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    path: Option<PathBuf>,
    /// Emit JSON to stdout instead of the human-readable table.
    #[arg(long)]
    json: bool,
    /// Filter by status (exact match). Common values: active, stopped, retired.
    #[arg(long)]
    status: Option<String>,
    /// Filter by backend (exact match).
    #[arg(long, value_enum)]
    backend: Option<spawn::Backend>,
    /// Filter to agents whose daemon is actually running (PID file + signal-0).
    #[arg(long)]
    running: bool,
    /// Filter to agents with at least one pending inbox envelope.
    #[arg(long = "inbox-pending")]
    inbox_pending: bool,
    /// Sort key. Default: registry insertion order. One of: id, inbox, incarnated, backend.
    #[arg(long)]
    sort: Option<String>,
    /// Print just the count of matching agents (one integer) instead of the table.
    /// With `--json`, emits `{"count": N}`.
    #[arg(long)]
    count: bool,
    /// Print bare agent ids, one per line. Combine with filters for `for $name in ...` loops.
    /// With `--json`, emits `{"names": [...]}`. `--count` wins if both are set.
    #[arg(long = "names-only")]
    names_only: bool,
}

impl ListArgs {
    fn into_runtime(self, lang: String) -> workspace::ListArgs {
        workspace::ListArgs {
            path: self.path,
            lang,
            json: self.json,
            status_filter: self.status,
            backend_filter: self.backend.map(|b| b.display_name().to_string()),
            running_only: self.running,
            inbox_pending_only: self.inbox_pending,
            sort: self.sort,
            count_only: self.count,
            names_only: self.names_only,
        }
    }
}

#[derive(Args, Debug)]
struct InitArgs {
    /// Path to initialize as a workspace. Defaults to current directory.
    path: Option<PathBuf>,
    /// Overwrite an existing workspace.toml.
    #[arg(long)]
    force: bool,
    /// Emit JSON `{ workspace, name, version, defaults, files_created }`
    /// instead of the human-readable creation report.
    #[arg(long)]
    json: bool,
}

impl InitArgs {
    fn into_runtime(self, lang: String) -> init::InitArgs {
        init::InitArgs {
            path: self.path,
            force: self.force,
            lang,
            json: self.json,
        }
    }
}

#[derive(Args, Debug)]
struct SpawnArgs {
    /// Path to the agent directory. Defaults to current directory.
    #[arg(long)]
    path: Option<PathBuf>,
    /// LLM backend CLI to invoke.
    #[arg(long, value_enum, default_value_t = spawn::Backend::Claude)]
    backend: spawn::Backend,
    /// Extra arguments passed verbatim to the backend CLI (after `--`).
    #[arg(last = true)]
    extra: Vec<OsString>,
}

impl SpawnArgs {
    fn into_runtime(self, lang: String) -> spawn::SpawnArgs {
        spawn::SpawnArgs {
            path: self.path,
            backend: self.backend,
            extra: self.extra,
            lang,
        }
    }
}

#[derive(Args, Debug)]
struct NewArgs {
    /// Agent name (kebab-case, e.g. "database-schema").
    name: String,
    /// Target directory for the new agent. Default: ../agent-<name>/ relative to template.
    #[arg(long)]
    target: Option<PathBuf>,
    /// Path to the template directory. Default: auto-detect `modules/agent-template/` from cwd ancestors.
    #[arg(long)]
    template: Option<PathBuf>,
    /// Primary backend recorded in the workspace registry. Default: claude.
    #[arg(long, value_enum, default_value_t = spawn::Backend::Claude)]
    backend: spawn::Backend,
    /// One-line role description. Prompted if missing on a TTY.
    #[arg(long)]
    role: Option<String>,
    /// Primary LLM model identifier. Prompted if missing on a TTY.
    #[arg(long)]
    primary_model: Option<String>,
    /// Fallback LLM model identifier (truly optional).
    #[arg(long)]
    fallback_model: Option<String>,
    /// File-based memory directory. Default: memories/
    #[arg(long, default_value = "memories/")]
    memory_path: String,
    /// Session data directory for Tier 2 mining (truly optional).
    #[arg(long)]
    sessions_path: Option<String>,
    /// Tier 2 memory CLI command (truly optional).
    #[arg(long)]
    deep_memory_cmd: Option<String>,
    /// Lint command for the verification gate. Prompted if missing on a TTY.
    #[arg(long)]
    lint_cmd: Option<String>,
    /// Format command for the verification gate. Prompted if missing on a TTY.
    #[arg(long)]
    format_cmd: Option<String>,
    /// Test command for the verification gate. Prompted if missing on a TTY.
    #[arg(long)]
    test_cmd: Option<String>,
    /// Build command for the verification gate. Prompted if missing on a TTY.
    #[arg(long)]
    build_cmd: Option<String>,
    /// Base directory for worktrees (truly optional). Default: /tmp
    #[arg(long)]
    worktree_base: Option<String>,
    /// Persona scope: one-line "this agent does X". Fills `{{scopeDescription}}`.
    #[arg(long)]
    scope: Option<String>,
    /// Persona anti-scope: one-line "this agent does NOT do Y".
    #[arg(long = "out-of-scope")]
    out_of_scope: Option<String>,
    /// Primary capability: longer description of what this agent is skilled at.
    /// Fills `{{primaryCapability}}`. Defaults to the role value when not provided.
    #[arg(long = "primary-capability")]
    primary_capability: Option<String>,
    /// Initial mindsets to seed — comma-separated kebab-case names (e.g.
    /// "verify-before-act,right-amount"). One stub `.md` per name.
    #[arg(long)]
    mindsets: Option<String>,
    /// Initial skills to seed — comma-separated kebab-case names. One stub
    /// `.md` per name.
    #[arg(long)]
    skills: Option<String>,
    /// Emit JSON `{ agent_id, target, registered_in, symlinks, mindset_stubs,
    /// skill_stubs, persona_filled }` instead of the human-readable report.
    /// Useful for scripted multi-agent setup.
    #[arg(long)]
    json: bool,
}

impl NewArgs {
    fn into_runtime(self, lang: String) -> new::NewArgs {
        new::NewArgs {
            name: self.name,
            target: self.target,
            template: self.template,
            backend: self.backend,
            lang,
            role: self.role,
            primary_model: self.primary_model,
            fallback_model: self.fallback_model,
            memory_path: self.memory_path,
            sessions_path: self.sessions_path,
            deep_memory_cmd: self.deep_memory_cmd,
            lint_cmd: self.lint_cmd,
            format_cmd: self.format_cmd,
            test_cmd: self.test_cmd,
            build_cmd: self.build_cmd,
            worktree_base: self.worktree_base,
            scope: self.scope,
            out_of_scope: self.out_of_scope,
            primary_capability: self.primary_capability,
            mindsets: self.mindsets,
            skills: self.skills,
            json: self.json,
        }
    }
}

#[derive(Args, Debug)]
struct SessionsArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit JSON `{ "sessions": [...] }` to stdout instead of the pretty table.
    #[arg(long)]
    json: bool,
    /// Seconds of inactivity before a session transitions from `working` to `idle`.
    #[arg(long = "idle-secs", default_value_t = 60)]
    idle_secs: u64,
}

impl From<SessionsArgs> for sessions::SessionsArgs {
    fn from(a: SessionsArgs) -> Self {
        Self {
            workspace: a.workspace,
            json: a.json,
            idle_secs: a.idle_secs,
        }
    }
}

fn main() -> ExitCode {
    // Detached background update-check refresh (issue #44): the parent spawns
    // us with this env set to do the network fetch + cache write off the hot
    // path. Short-circuit before arg parsing and every other hook so the child
    // does no user-visible work, then exit.
    if std::env::var_os(update::REFRESH_ENV).is_some() {
        update::run_background_refresh();
        return ExitCode::SUCCESS;
    }

    let cli = Cli::parse();
    let lang = resolve_lang(cli.lang);
    let bundle = i18n::bundle_for(&lang);

    // Best-effort: ensure ~/.bwoc/ exists before any command runs. Failure here
    // (e.g., $HOME unset or read-only filesystem) logs a warning but does not
    // block the command — most commands don't yet read user-level config.
    if let Err(e) = user_home::ensure_initialized() {
        eprintln!("bwoc: warning — could not initialize ~/.bwoc/: {e}");
    }

    // One-line "you upgraded" notice on subcommands (once per MAJOR.MINOR).
    // The bare-`bwoc` banner already carries the full What's New block, so
    // skip the notice there to avoid doubling up.
    if cli.command.is_some() {
        whats_new::notify_if_updated();
        // Opportunistic "newer release available" notice (issue #44). Reads the
        // throttle cache; refreshes in a detached child at most once / 24h.
        update::notify_if_drifted(matches!(cli.command, Some(Commands::Update { .. })));
    }

    match cli.command {
        Some(Commands::Check {
            path,
            all,
            workspace,
            json,
        }) => {
            let code = if all {
                check::run_all(workspace.as_deref(), &lang, json)
            } else {
                let target = path.unwrap_or_else(|| PathBuf::from("."));
                check::run(&target, &lang, json)
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::New(args)) => {
            let code = new::run((*args).into_runtime(lang.clone()));
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Spawn(args)) => {
            let code = spawn::run(args.into_runtime(lang.clone()));
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Init(args)) => {
            let code = init::run(args.into_runtime(lang.clone()));
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Workspace { command }) => {
            let code = match command {
                WorkspaceCommand::Info {
                    path,
                    json,
                    path_only,
                } => workspace::run_info(workspace::InfoArgs {
                    path,
                    lang: lang.clone(),
                    json,
                    path_only,
                }),
                WorkspaceCommand::Validate { path, json } => {
                    workspace::run_validate(workspace::ValidateArgs {
                        path,
                        lang: lang.clone(),
                        json,
                    })
                }
                WorkspaceCommand::Prune { path, apply, json } => {
                    workspace::run_prune(workspace::PruneArgs { path, apply, json })
                }
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::List(args)) => {
            let code = workspace::run_list(args.into_runtime(lang.clone()));
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Doctor(args)) => {
            let code = doctor::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Retire(args)) => {
            let code = retire::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Status(args)) => {
            let mut runtime: status::StatusArgs = args.into();
            runtime.lang = lang.clone();
            let code = status::run(runtime);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Help(args)) => {
            let code = help::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Completion(args)) => {
            let code = completion::run::<Cli>(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Dashboard(args)) => {
            let code = dashboard::run(args.into_runtime(lang.clone()));
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Stop(args)) => {
            let code = stop::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Start(args)) => {
            let code = start::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Ping(args)) => {
            let code = ping::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Supervise(args)) => {
            let code = supervise::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Trust(args)) => {
            let code = trust::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Send(args)) => {
            let runtime = match args.resolve_message() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("bwoc send: {e}");
                    return ExitCode::from(2);
                }
            };
            let code = send::run(runtime);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Chat(args)) => {
            let code = chat::run(args.into_runtime(lang.clone()));
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Run(args)) => {
            let code = run::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Inbox(args)) => {
            let code = inbox::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Log(args)) => {
            let code = log::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Memory(action)) => {
            // Tier 2 variants are dispatched to deep_memory_cmd; all others
            // go to the existing Tier 1 memory module unchanged.
            let code = match action {
                MemoryAction::WakeUp { agent, workspace } => {
                    deep_memory_cmd::run(deep_memory_cmd::Tier2Args {
                        action: deep_memory_cmd::Tier2Action::WakeUp,
                        agent,
                        workspace,
                    })
                }
                MemoryAction::T2Search {
                    query,
                    agent,
                    workspace,
                } => deep_memory_cmd::run(deep_memory_cmd::Tier2Args {
                    action: deep_memory_cmd::Tier2Action::Search { query },
                    agent,
                    workspace,
                }),
                MemoryAction::Mine {
                    path,
                    agent,
                    mode,
                    workspace,
                } => deep_memory_cmd::run(deep_memory_cmd::Tier2Args {
                    action: deep_memory_cmd::Tier2Action::Mine { path, mode },
                    agent,
                    workspace,
                }),
                tier1_action => memory::run(tier1_action.into_runtime()),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Team(command)) => {
            let code = match command {
                TeamCommand::Create {
                    id,
                    members,
                    workspace,
                    json,
                } => sangha::run_team_create(workspace, id, members, json),
                TeamCommand::List { workspace, json } => sangha::run_team_list(workspace, json),
                TeamCommand::Retire {
                    id,
                    workspace,
                    yes,
                    json,
                } => sangha::run_team_retire(workspace, id, yes, json),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Task(command)) => {
            let code = match command {
                TaskCommand::Add {
                    team,
                    title,
                    deps,
                    id,
                    requires_plan,
                    workspace,
                    json,
                } => sangha::run_task_add(workspace, team, title, deps, id, requires_plan, json),
                TaskCommand::List {
                    team,
                    workspace,
                    json,
                } => sangha::run_task_list(workspace, team, json),
                TaskCommand::Claim {
                    team,
                    task,
                    agent,
                    workspace,
                    json,
                } => sangha::run_task_claim(workspace, team, task, agent, json),
                TaskCommand::Complete {
                    team,
                    task,
                    agent,
                    workspace,
                    json,
                } => sangha::run_task_complete(workspace, team, task, agent, json),
                TaskCommand::Plan {
                    team,
                    task,
                    agent,
                    plan,
                    plan_file,
                    workspace,
                    json,
                } => {
                    // Resolve plan content: --plan inline, or --plan-file body.
                    let plan_content = match (plan, plan_file) {
                        (Some(p), _) => Some(p),
                        (None, Some(path)) => match std::fs::read_to_string(&path) {
                            Ok(s) => Some(s.trim_end_matches('\n').to_string()),
                            Err(e) => {
                                eprintln!("bwoc task plan: failed to read {}: {e}", path.display());
                                return ExitCode::from(2);
                            }
                        },
                        (None, None) => None,
                    };
                    sangha::run_task_plan(workspace, team, task, agent, plan_content, json)
                }
                TaskCommand::Approve {
                    team,
                    task,
                    workspace,
                    json,
                } => sangha::run_task_review(workspace, team, task, true, json),
                TaskCommand::Reject {
                    team,
                    task,
                    workspace,
                    json,
                } => sangha::run_task_review(workspace, team, task, false, json),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Peer(sub)) => {
            let code = match sub {
                PeerCommand::List { workspace } => peer::run(peer::PeerArgs {
                    action: peer::PeerAction::List,
                    workspace,
                }),
                PeerCommand::View { key, workspace } => peer::run(peer::PeerArgs {
                    action: peer::PeerAction::View { key },
                    workspace,
                }),
                PeerCommand::Learn {
                    key,
                    doc,
                    workspace,
                } => peer::run(peer::PeerArgs {
                    action: peer::PeerAction::Learn { key, doc },
                    workspace,
                }),
                // Give-feedback (#20) is a signed cross-workspace send with a
                // `feedback` kind — reuse the send path (routing + signing +
                // delivery) rather than duplicating it.
                PeerCommand::Feedback {
                    agent,
                    message,
                    from,
                    workspace,
                } => send::run(send::SendArgs {
                    to: agent,
                    message,
                    from: Some(from),
                    reply_to: None,
                    // Local tmux wakeup is meaningless for a cross-workspace
                    // recipient (and could ping an unrelated local session).
                    no_wakeup: true,
                    workspace,
                    kind: Some("feedback".to_string()),
                    // Route to the peer (not a same-named local agent), and
                    // require a signature — feedback must be verifiable.
                    force_peer_route: true,
                    require_signed: true,
                }),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Sessions(args)) => {
            let code = sessions::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Update { check, run }) => {
            let code = update::run(update::UpdateArgs { check, run });
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Notes(sub)) => {
            let code = dispatch_doc_cmd("notes", sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Retro(sub)) => {
            let code = dispatch_doc_cmd("retrospectives", sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Research(sub)) => {
            let code = dispatch_doc_cmd("research", sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Doc(sub)) => {
            let code = dispatch_doc_kind_cmd(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Fleet(sub)) => {
            let code = match sub {
                FleetCommand::Health(args) => fleet::run(fleet::FleetHealthArgs {
                    workspace: args.workspace,
                    json: args.json,
                    stale_days: args.stale_days,
                }),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Skill(sub)) => {
            let code = match sub {
                SkillCommand::List(args) => skill::run_list(skill::ListArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    enabled: args.enabled,
                    agent: args.agent,
                    json: args.json,
                }),
                SkillCommand::Show(args) => skill::run_show(skill::ShowArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    json: args.json,
                }),
                SkillCommand::Verify(args) => skill::run_verify(skill::VerifyArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    all: args.all,
                    run_gates: args.run_gates,
                    json: args.json,
                }),
                SkillCommand::Init(args) => skill::run_init(skill::InitArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    version: args.version,
                    description: args.description,
                    operation: args.operation,
                    json: args.json,
                }),
                SkillCommand::Install(args) => skill::run_install(skill::InstallArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    source: args.source,
                    no_verify: args.no_verify,
                    allow_new_source: args.allow_new_source,
                    upgrade: args.upgrade,
                    json: args.json,
                }),
                SkillCommand::Enable(args) => skill::run_enable(skill::EnableArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    agent: args.agent,
                    json: args.json,
                }),
                SkillCommand::Disable(args) => skill::run_disable(skill::DisableArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    agent: args.agent,
                    json: args.json,
                }),
                SkillCommand::Remove(args) => skill::run_remove(skill::RemoveArgs {
                    common: skill::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    yes: args.yes,
                    forget_source: args.forget_source,
                    json: args.json,
                }),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Plugin(sub)) => {
            let code = match sub {
                PluginCommand::List(args) => plugin::run_list(plugin::ListArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    enabled: args.enabled,
                    kind: args.kind,
                    json: args.json,
                }),
                PluginCommand::Show(args) => plugin::run_show(plugin::ShowArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    json: args.json,
                }),
                PluginCommand::Init(args) => plugin::run_init(plugin::InitArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    kind: args.kind,
                    version: args.version,
                    description: args.description,
                    json: args.json,
                }),
                PluginCommand::Install(args) => plugin::run_install(plugin::InstallArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    source: args.source,
                    no_verify: args.no_verify,
                    allow_new_source: args.allow_new_source,
                    upgrade: args.upgrade,
                    json: args.json,
                }),
                PluginCommand::Enable(args) => plugin::run_enable(plugin::EnableArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    json: args.json,
                }),
                PluginCommand::Disable(args) => plugin::run_disable(plugin::DisableArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    json: args.json,
                }),
                PluginCommand::Remove(args) => plugin::run_remove(plugin::RemoveArgs {
                    common: plugin::CommonArgs {
                        workspace: args.workspace,
                    },
                    name: args.name,
                    yes: args.yes,
                    forget_source: args.forget_source,
                    json: args.json,
                }),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Audit(sub)) => {
            let code = match sub {
                AuditCommand::Run(args) => audit::run(audit::RunArgs {
                    common: audit::CommonArgs {
                        workspace: args.workspace,
                    },
                    plugin: args.plugin,
                    json: args.json,
                }),
            };
            // audit::run uses 255 for framework error; clap's ExitCode is u8,
            // so any code we hand out fits — `as u8` would also be fine but
            // u8::try_from + unwrap_or(1) matches sibling dispatch arms.
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Jira(sub)) => {
            // jira owns its own clap arg structs (jira.rs) so arg parsing is
            // unit-testable; dispatch is a thin hand-off. Exit-code convention
            // is documented in jira.rs (0/1/2/3/4/255).
            let code = jira::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::A2a(sub)) => {
            let code = match sub {
                A2aCommand::Card(args) => a2a::run_card(a2a::CardArgs {
                    agent: args.agent,
                    workspace: args.workspace,
                }),
                A2aCommand::Serve(args) => a2a::run_serve(a2a::ServeArgs {
                    agent: args.agent,
                    workspace: args.workspace,
                    bind: args.bind,
                    port: args.port,
                    team: args.team,
                }),
                A2aCommand::FetchCard(args) => {
                    a2a::run_fetch_card(a2a::FetchCardArgs { url: args.url })
                }
                A2aCommand::Send(args) => a2a::run_send_outbound(a2a::SendOutboundArgs {
                    url: args.url,
                    message: args.message,
                    context: args.context,
                }),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        None => {
            // No subcommand — print the startup banner. Banner already
            // includes a `bwoc --help` hint at the bottom.
            let _ = &bundle; // banner is lang-agnostic for now
            banner::print();
            ExitCode::SUCCESS
        }
    }
}

fn resolve_lang(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("BWOC_LANG").ok())
        .or_else(|| std::env::var("LANG").ok().and_then(parse_locale))
        .unwrap_or_else(|| "en".to_string())
}

/// Extract the language tag from a POSIX `LANG`-style value like `th_TH.UTF-8`.
fn parse_locale(raw: String) -> Option<String> {
    let tag = raw.split(['.', '@']).next()?;
    let lang = tag.split('_').next()?;
    if lang.is_empty() {
        None
    } else {
        Some(lang.to_ascii_lowercase())
    }
}

/// Dispatch a `DocSubcommand` for the named built-in kind.
/// Resolves the workspace root, looks up the `DocKind`, and runs the generic engine.
fn dispatch_doc_cmd(kind_name: &str, sub: DocSubcommand) -> i32 {
    use bwoc_core::doc_kind::kind as lookup;

    let Some(k) = lookup(kind_name) else {
        eprintln!("bwoc: unknown document kind '{kind_name}' (internal error)");
        return 2;
    };

    let (action, workspace_opt) = match sub {
        DocSubcommand::New { title, workspace } => (doc_cmd::DocAction::New { title }, workspace),
        DocSubcommand::List { workspace } => (doc_cmd::DocAction::List, workspace),
        DocSubcommand::View { name, workspace } => (doc_cmd::DocAction::View { name }, workspace),
    };

    let root = resolve_doc_workspace(workspace_opt);
    doc_cmd::run(k, action, &root)
}

/// Dispatch a generic `bwoc doc <kind> <action>` invocation.
///
/// Resolves the kind against built-ins first, then workspace-declared custom
/// kinds from `.bwoc/doc-kinds.toml`.  Emits a clear error (listing available
/// kinds) on unknown kind names.
fn dispatch_doc_kind_cmd(sub: DocKindSubcommand) -> i32 {
    use bwoc_core::doc_kind::resolve_kind;

    let (kind_name, action, workspace_opt) = match sub {
        DocKindSubcommand::New {
            kind,
            title,
            workspace,
        } => (kind, doc_cmd::DocAction::New { title }, workspace),
        DocKindSubcommand::List { kind, workspace } => (kind, doc_cmd::DocAction::List, workspace),
        DocKindSubcommand::View {
            kind,
            name,
            workspace,
        } => (kind, doc_cmd::DocAction::View { name }, workspace),
    };

    let root = resolve_doc_workspace(workspace_opt);

    match resolve_kind(&kind_name, &root) {
        Ok(k) => doc_cmd::run(k, action, &root),
        Err(msg) => {
            eprintln!("bwoc doc: {msg}");
            1
        }
    }
}

/// Workspace resolution for document-kind commands.
/// Resolution: explicit arg → BWOC_WORKSPACE env → ancestor walk → cwd fallback.
fn resolve_doc_workspace(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }
    if let Ok(mut cur) = std::env::current_dir() {
        loop {
            if cur.join(".bwoc/workspace.toml").is_file() {
                return cur.clone();
            }
            if !cur.pop() {
                break;
            }
        }
    }
    // Final fallback: current directory (notes will live relative to cwd).
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
