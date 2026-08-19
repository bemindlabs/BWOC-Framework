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
mod accounting;
mod agent_run;
mod audit;
mod banner;
mod chat;
mod check;
mod completion;
mod council;
mod dashboard;
mod debase;
mod deep_memory_cmd;
mod digest;
mod doc_cmd;
mod doctor;
mod eval;
mod figma;
mod fleet;
mod fleet_term;
mod gcloud;
mod git_worktree;
mod gws;
mod handbook;
mod help;
mod i18n;
mod inbox;
mod info;
mod init;
mod jira;
mod livecheck;
mod log;
mod loop_cmd;
mod memory;
mod monitor;
mod new;
mod okr;
mod outbox_cmd;
mod peer;
mod ping;
mod plugin;
mod receipts;
mod remote;
mod report;
mod resource;
mod retire;
mod run;
mod sangha;
mod send;
mod sessions;
mod set;
mod skill;
mod spawn;
mod start;
mod status;
mod stop;
mod supervise;
mod tasks;
mod triage;
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
    /// Run one eval fixture through `bwoc-harness` against a backend and report
    /// the score (exit 0 = pass/skip, 1 = fail). `--json` for structured output.
    Eval(EvalCliArgs),
    /// Retire an agent — remove it from the workspace's registry (vaya).
    Retire(RetireArgs),
    /// Update an incarnated agent's backend and/or model in place (CRUD update).
    Set {
        /// Agent name (with or without the `agent-` prefix).
        name: String,
        /// New backend — rewrites the `.bwoc/agents.toml` entry. All backend
        /// symlinks already exist, so no file changes are needed beyond it.
        #[arg(long, value_enum)]
        backend: Option<spawn::Backend>,
        /// New `primaryModel` in `config.manifest.json` (a model id, or "auto").
        #[arg(long)]
        primary_model: Option<String>,
        /// New `fallbackModel` in `config.manifest.json` (metadata only —
        /// shown in status/dashboards, not a runtime fallback).
        #[arg(long)]
        fallback_model: Option<String>,
        /// Workspace root (defaults: this flag > BWOC_WORKSPACE > ancestor walk).
        #[arg(long)]
        workspace: Option<std::path::PathBuf>,
        /// Emit JSON instead of the human-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Manage the agent → base-project binding (worktreeBase): list / show / set.
    Debase(DebaseArgs),
    /// Link agents to remote-control sessions and manage them: link / list / status / unlink.
    Remote(RemoteArgs),
    /// Show per-agent health + identity snapshot (read-only).
    Status(StatusArgs),
    /// Topic-specific help (backends, workspace, manifest, arc, getting-started).
    Help(HelpArgs),
    /// Bundled offline quick guide. `bwoc handbook` lists sections; `bwoc
    /// handbook <section>` prints one (start, agents, spawn, teams, harness, release).
    Handbook(HandbookCliArgs),
    /// One-card system status: version, release, phase, workspace + agents, update.
    Info(InfoCliArgs),
    /// Report a bug/feature/question to the BWOC issue tracker (preview +
    /// confirm; falls back to a prefilled browser URL when unattended).
    Report(ReportCliArgs),
    /// Emit a shell completion script (bash, zsh, fish, powershell, elvish).
    Completion(CompletionArgs),
    /// Launch the interactive TUI dashboard (agents list with navigation; refresh with `r`).
    Dashboard(DashboardArgs),
    /// Launch the Loop-Engineering control center (L1 goal-loop): watch a team's
    /// task list drive toward Definition-of-Done. TUI; `--team` opens one directly.
    Loop(LoopArgs),
    /// Watch a source and alert on transition (Loop-Engineering L3). Probe
    /// `--exec` each tick (exit 0 = OK, non-zero = tripped); alert `--alert
    /// <agent>` once per OK↔TRIP transition. `--loop` runs it continuously.
    Monitor(MonitorArgs),
    /// Deliver a recurring digest (Loop-Engineering L3): run `--exec` **exactly
    /// once per `--period`** (durable across restarts) and emit its output to
    /// stdout or `--out <file>`. `--loop` runs it continuously.
    Digest(DigestArgs),
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
    /// Agent host operations (e.g. launch a session as an unprivileged user on a
    /// root-only VPS — `bwoc agent run --as-user <user> <agent>`).
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Read messages from an agent's inbox (`.bwoc/inbox.jsonl`).
    Inbox(InboxArgs),
    /// Rule-based coordinator loop over an agent's gateway inbox: classify each
    /// message (ack / escalate / forward) without running a model, advance a
    /// cursor (exactly-once), and digest. No LLM — safe for ambient backends.
    Triage(TriageArgs),
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
    /// Query task status across **every** team (fleet-wide): filter by `--agent`
    /// (claimant) / `--state` (pending|in_progress|completed), table or `--json`.
    Tasks(TasksCliArgs),
    /// Query read receipts — "was my message consumed?" — across recipients'
    /// triage logs: filter by `--message-id` / `--from` / `--agent`, table or `--json` (#299).
    Receipts(ReceiptsCliArgs),
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
    /// Fleet status + governance. Bare `bwoc fleet` shows the status overview;
    /// `health` runs the Aparihāniya-dhamma 7 signals (read-only, report-only).
    #[command(name = "fleet")]
    Fleet(FleetArgs),
    /// Sender-side durable retry queue. `bwoc outbox` (or `list`) shows messages
    /// spooled because a remote peer was offline; `flush` retries delivery.
    #[command(name = "outbox")]
    Outbox(OutboxArgs),
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
    /// `BWOC_JIRA_{EMAIL,TOKEN,BASE_URL}` env, or the `[jira]` table of the
    /// gitignored `0600` `.bwoc/secrets.toml` when unset (never committed); a
    /// missing one exits 2. The live REST path is provided by a `jira`-kind plugin
    /// (see PLUGINS.en.md §jira); without one, live verbs stub-error (exit 4).
    /// Every verb has a `--json` twin. See `docs/en/PLUGINS.en.md`.
    #[command(name = "jira", subcommand)]
    Jira(jira::JiraCommand),

    /// Operate the `workflow/gcloud-*` plugins (BWOC-EPIC-8 foundation): show
    /// active credential source + account email (`auth status`), list / show /
    /// `set-default` projects (`project ...`), or get the combined view
    /// (`status`). Read-mostly; `project set-default` is a gated write to the
    /// local `gcloud` config. Live verbs dispatch to `workflow/gcloud-auth` and
    /// `workflow/gcloud-project` plugins; without one, the live verb exits 4
    /// (`status` keeps working offline). Every verb has a `--json` twin. See
    /// `notes/2026-05-28_gcloud-workflow-plugin-architecture.md`.
    #[command(name = "gcloud", subcommand)]
    Gcloud(gcloud::GcloudCommand),

    /// Operate the `okr` plugin kind (BWOC-EPIC-4): list installed okr plugins,
    /// show a plugin's SPEC + objectives summary, `track` a key result's current
    /// value (a local-file write to the operator's own `key_results.toml`, so
    /// no confirmation gate), or `report` the OKR Progress Schema JSON for every
    /// key result. Verbs dispatch to an installed `okr`-kind plugin (e.g.
    /// `workspace-okrs`, BWOC-49); a missing plugin exits 4 with an install hint.
    /// Every verb has a `--json` twin. See `docs/en/PLUGINS.en.md` §OKR Progress
    /// Schema + `notes/2026-05-28_okr-plugin-architecture.md`.
    #[command(name = "okr", subcommand)]
    Okr(okr::OkrCommand),

    /// Operate the `council` plugin kind (BWOC-EPIC-5): the framework's first
    /// *coordination* kind. `propose` opens a decision (question + options,
    /// participants drawn from a `bwoc team`); `discuss` adds a round turn,
    /// routing it through the inbox transport (`bwoc send`) and recording the
    /// envelope ref; `vote` appends an (append-only) vote; `resolve` tallies per
    /// the plugin's declared voting model, checks quorum, and records the
    /// outcome + any dissent (or abandons on quorum failure); `list`/`show` read.
    /// The voting model + quorum come from the named `council`-kind plugin's
    /// `[council]` manifest table (e.g. `council-sangha-7`, BWOC-59); a missing
    /// plugin exits 4. Every verb has a `--json` twin. See
    /// `docs/en/PLUGINS.en.md` §Council Decision Schema +
    /// `notes/2026-05-28_council-plugin-architecture.md`.
    #[command(name = "council", subcommand)]
    Council(council::CouncilCommand),

    /// Operate the `figma` plugin kind (BWOC-EPIC-7): a read-mostly Figma REST
    /// integration that bridges design→dev. `fetch` pulls frame/node metadata
    /// (Figma Asset Mapping Schema shape); `export` renders a node image into the
    /// content-addressable cache under `figma/exports/` and returns its path;
    /// `tokens` extracts design tokens; `status` reports the auth state (token
    /// present? which scope?) + rate-limit headroom — never the token value. The
    /// live REST path is provided by a named `figma`-kind plugin (e.g.
    /// `figma-rest`, BWOC-64); a missing plugin exits 4. Auth resolves from the
    /// `BWOC_FIGMA_TOKEN` env (the CLI only checks presence, never logs or
    /// serializes it); the read verbs exit 2 when it is absent. Every verb has a
    /// `--json` twin. See `docs/en/PLUGINS.en.md` §Figma Asset Mapping Schema +
    /// `notes/2026-05-28_figma-plugin-architecture.md`.
    #[command(name = "figma", subcommand)]
    Figma(figma::FigmaCommand),

    /// Operate the `gws` plugin kind (BWOC-EPIC-13): a read-mostly Google
    /// Workspace integration. `auth status` reports OAuth token presence + granted
    /// scopes + account (never the token value); `drive list`/`show` read Drive
    /// files; `gmail search`/`show`/`labels` read mail threads + labels;
    /// `calendar list`/`events` read calendars + events. Resources follow the
    /// Workspace Resource Schema. Live verbs dispatch to the per-service
    /// `gws-{drive,gmail,calendar}` plugins (sourcing the `gws-auth` foundation,
    /// BWOC-75/76); a missing plugin exits 4. Auth resolves from the
    /// `BWOC_GWS_TOKEN` env or `.bwoc/secrets/gws-token.json` (the CLI only checks
    /// presence, never logs or serializes it); the read verbs exit 2 when no token
    /// is present. `--max` caps an otherwise unbounded list. Every verb has a
    /// `--json` twin. See `docs/en/PLUGINS.en.md` §Workspace Resource Schema +
    /// `notes/2026-05-28_google-workspace-plugin-architecture.md`.
    #[command(name = "gws", subcommand)]
    Gws(gws::GwsCommand),

    /// Bemind Accounting Open API — read financial reports (free) and record
    /// purchases/expenses (gated). Fronts the `workflow/accounting-api` plugin;
    /// every financial write needs `[plugins.accounting-api] writes_enabled =
    /// true` in `.bwoc/workspace.toml` plus a per-write confirm (or `--yes`,
    /// required in `--json`). The plugin holds no gate — this is the single
    /// choke point. See `docs/en/PLUGINS.en.md` §Write verbs.
    #[command(name = "accounting", subcommand)]
    Accounting(accounting::AccountingCommand),

    /// Fleet compute & memory sharing (Resource Protocol). Borrow GPU/CPU/RAM,
    /// shared KV, or federated knowledge from another fleet host through the
    /// `bwoc-gateway` broker, under a provider-side sharing gate. Slice A ships
    /// the local `snapshot` + `gate-check` verbs; advertise/discover/claim land
    /// with the broker. See `docs/en/RESOURCE-PROTOCOL.en.md`.
    #[command(name = "resource", subcommand)]
    Resource(resource::ResourceCommand),

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
    /// Bearer token to present (overrides the per-origin entry in
    /// `<workspace>/.bwoc/a2a-credentials.json`).
    #[arg(long)]
    token: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
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
    /// Bearer token to present (overrides the per-origin entry in
    /// `<workspace>/.bwoc/a2a-credentials.json`). Presented only if the peer's
    /// card declares a Bearer scheme.
    #[arg(long)]
    token: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
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
    /// Address to bind. Defaults to loopback (`127.0.0.1`). A non-loopback bind
    /// requires an auth token (`BWOC_A2A_TOKEN` / `.bwoc/a2a.token`) or
    /// `--allow-unauthenticated`, otherwise the listener refuses to start.
    #[arg(long, default_value = "127.0.0.1")]
    bind: std::net::IpAddr,
    /// TCP port to listen on.
    #[arg(long, default_value_t = 41241)]
    port: u16,
    /// Expose this team's shared task list over A2A `tasks/*` (GetTask/ListTasks).
    #[arg(long)]
    team: Option<String>,
    /// Permit a non-loopback bind with NO auth token (serve with a loud warning
    /// instead of refusing). For trusted networks or a front proxy that adds
    /// auth; never expose an unauthenticated listener to an untrusted network.
    #[arg(long)]
    allow_unauthenticated: bool,
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
    /// Plugin kind — one of the kinds in `check::PLUGIN_KINDS` (PLUGINS.en.md
    /// §"Plugin Kinds"): `memory-backend`, `llm-backend`, `workflow`, `audit`,
    /// `jira`, `okr`, `council`, `figma`, `gws`.
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

#[derive(Args, Debug)]
struct FleetArgs {
    /// Subcommand. Absent → the status overview (issue #297).
    #[command(subcommand)]
    command: Option<FleetCommand>,
}

#[derive(Args, Debug)]
struct OutboxArgs {
    /// Subcommand. Absent → `list`.
    #[command(subcommand)]
    command: Option<OutboxCommand>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace", global = true)]
    workspace: Option<PathBuf>,
}

#[derive(clap::Subcommand, Debug)]
enum OutboxCommand {
    /// Show pending messages per peer (the default).
    List,
    /// Retry delivery of spooled messages; `--peer` limits it to one recipient.
    Flush {
        /// Only flush this recipient's queue (id or bare name). Omit → all peers.
        #[arg(long)]
        peer: Option<String>,
    },
}

#[derive(clap::Subcommand, Debug)]
enum FleetCommand {
    /// Per-agent status overview: backend, status, pending inbox count, last-seen
    /// (table or `--json`, non-TTY friendly). The default when no subcommand given.
    Status(FleetStatusArgs),
    /// Check all 7 Aparihāniya-dhamma fleet-governance signals (read-only).
    Health(FleetHealthArgs),
    /// Open a terminal for every agent in the fleet — one tmux pane each,
    /// arranged by `--layout`. Portable across macOS + Linux (uses tmux).
    Term(FleetTermArgs),
}

#[derive(Args, Debug)]
struct FleetTermArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Pane arrangement: grid (default) | columns | rows | main-vertical | main-horizontal.
    /// Cycle layouts live inside tmux with `<prefix> Space`.
    #[arg(long, value_enum, default_value_t = fleet_term::TmuxLayout::Grid)]
    layout: fleet_term::TmuxLayout,
    /// tmux session name to create. Omit for a per-workspace default
    /// (`bwoc-fleet-<workspace>-<hash>`) so concurrent fleets don't collide.
    #[arg(long)]
    session: Option<String>,
    /// Build the session but don't attach — just print the attach command
    /// (implied when stdout is not a TTY).
    #[arg(long)]
    print: bool,
}

#[derive(Args, Debug)]
struct FleetStatusArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON object instead of the human table.
    #[arg(long)]
    json: bool,
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
    /// Goal-loop mode (Loop-Engineering L2): re-scan on a ticker and auto-remediate
    /// stale PID/socket warns (`doctor --auto`) until all conditions are green, a
    /// non-remediable warn remains (operator action), or the budget. Reconcile loop.
    /// Conflicts with `--json` (the loop emits human progress + `doctor` output).
    #[arg(long = "loop", conflicts_with = "json")]
    loop_mode: bool,
    /// Ticker interval (seconds) between fleet-health fires. Only with `--loop`.
    #[arg(long, default_value_t = 30, requires = "loop_mode")]
    loop_interval_secs: u64,
    /// Iteration budget so the loop provably halts. `0` = unbounded. Only with `--loop`.
    #[arg(long, default_value_t = 20, requires = "loop_mode")]
    loop_max_iters: usize,
}

#[derive(clap::Subcommand, Debug)]
enum AgentCommand {
    /// Launch an agent session as an unprivileged user (root-only VPS pattern,
    /// #322). Must be run as root; drops to `--as-user` via `runuser` and runs
    /// in the agent's directory. Without a trailing `-- <cmd…>` it runs
    /// `bwoc-agent --serve`; with one, it runs that (e.g. a remote-control CLI).
    Run(AgentRunCliArgs),
}

#[derive(Args, Debug)]
struct AgentRunCliArgs {
    /// Agent id or bare name (e.g. `pi` or `agent-pi`).
    agent: String,
    /// The unprivileged user to drop to (must already exist).
    #[arg(long = "as-user")]
    as_user: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Command to run as the user (after `--`). Empty → `bwoc-agent --serve`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

impl From<AgentRunCliArgs> for agent_run::AgentRunArgs {
    fn from(a: AgentRunCliArgs) -> Self {
        Self {
            agent: a.agent,
            as_user: a.as_user,
            workspace: a.workspace,
            command: a.command,
        }
    }
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
    /// Print the resolved inbox path for `<agent>` and exit (no read). Lets an
    /// external writer derive the same path the CLI reads, e.g.
    /// `--inbox "$(bwoc inbox agent-foo --path)"` (issue #302). Needs a single
    /// agent — conflicts with `--all`.
    #[arg(long, conflicts_with = "all")]
    path: bool,
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
            path: a.path,
        }
    }
}

#[derive(Args, Debug)]
struct TriageArgs {
    /// Agent whose inbox to triage. Matches by id ("agent-foo") or bare name.
    agent: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON digest instead of the human one.
    #[arg(long)]
    json: bool,
    /// Poll continuously after draining the backlog (Ctrl-C to stop). Without
    /// it, drain the current backlog once and exit.
    #[arg(long = "loop")]
    loop_: bool,
    /// Classify + digest only: no receipts, no forwarding, cursor not advanced.
    #[arg(long = "dry-run")]
    dry_run: bool,
}

impl From<TriageArgs> for triage::TriageArgs {
    fn from(a: TriageArgs) -> Self {
        Self {
            agent: a.agent,
            workspace: a.workspace,
            json: a.json,
            loop_: a.loop_,
            dry_run: a.dry_run,
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
    /// Run `bwoc spawn` under tmux instead of exec'ing in this shell. Adds a
    /// window when already inside tmux; otherwise auto-starts a `bwoc-<id>`
    /// session.
    #[arg(long, conflicts_with = "ghostty")]
    tmux: bool,
    /// Open a new Ghostty terminal window instead of exec'ing in this shell. macOS-only.
    #[arg(long)]
    ghostty: bool,
    /// Full-screen ratatui chat client. Spawns `bwoc-harness --chat` and renders
    /// the chat_proto event stream. Harness backends only (ollama /
    /// openai-compatible); other backends print a hint and fall back to exec.
    #[arg(long, conflicts_with_all = ["tmux", "ghostty"])]
    tui: bool,
    /// Join a team's shared chat channel (HV3-3a): teammate replies are injected
    /// into context and this agent's replies broadcast to the team. Requires
    /// `--tui` with a harness backend; the agent must be a team member.
    #[arg(long)]
    team: Option<String>,
    /// Open the multi-agent **fleet** TUI: a left sidebar of every agent, `Tab`
    /// to switch, one live session per agent. Requires `--tui` with a harness
    /// backend; the named agent's backend/model seed the shared session config.
    #[arg(long, requires = "tui")]
    fleet: bool,
}

impl ChatArgs {
    fn into_runtime(self, lang: String) -> chat::ChatArgs {
        chat::ChatArgs {
            name: self.name,
            workspace: self.workspace,
            lang,
            tmux: self.tmux,
            ghostty: self.ghostty,
            tui: self.tui,
            team: self.team,
            fleet: self.fleet,
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
    /// Directory to run the agent in. Default: the agent's own directory (jailed).
    /// Pass a path (relative resolves against the workspace root; `.` = workspace
    /// root) to let the task touch shared files like `projects/` or `wiki/`. The
    /// resolved path must be an existing directory inside the workspace.
    #[arg(long = "workdir")]
    workdir: Option<PathBuf>,
}

impl From<RunCliArgs> for run::RunArgs {
    fn from(a: RunCliArgs) -> Self {
        Self {
            agent: a.agent,
            task: a.task,
            json: a.json,
            timeout_secs: a.timeout,
            workspace: a.workspace,
            workdir: a.workdir,
        }
    }
}

#[derive(Args, Debug)]
// `body` is non-required at the clap layer because in broadcast mode
// (`--all` / `--team`) the message text lands in the `to` positional (clap
// fills positional 1 first). `resolve` enforces "a message is present" for
// both modes with an actionable error. `body` still keeps message⊕file
// mutually exclusive.
#[command(group(clap::ArgGroup::new("body").required(false).args(["message", "file"])))]
// clap renders the `body` group ahead of the plain `to` positional, producing a
// usage line that contradicts the actual parse order (`to` is positional 1,
// `message` is positional 2). Pin the usage string to reality so the help text
// doesn't mislead. (Yoniso Manasikāra — the docs match the behaviour, not
// clap's default rendering.)
#[command(override_usage = "bwoc send [OPTIONS] <TO|--team <TEAM>|--all> <MESSAGE|--file <FILE>>")]
struct SendArgs {
    /// Recipient agent. Matches by id ("agent-foo") or bare name ("foo").
    /// Omit when broadcasting with `--all` / `--team` (the positional then
    /// carries the message).
    to: Option<String>,
    /// Message text (quote multi-word). Mutually exclusive with `--file`.
    message: Option<String>,
    /// Source file. The file's full contents (minus trailing newlines) becomes
    /// the message body. Mutually exclusive with `<message>`.
    #[arg(long)]
    file: Option<PathBuf>,
    /// Broadcast to every member of this Saṅgha team (`.bwoc/teams/<id>.toml`).
    /// Mutually exclusive with a `<TO>` recipient and with `--all`.
    #[arg(long)]
    team: Option<String>,
    /// Broadcast to every agent registered in the workspace. Mutually exclusive
    /// with a `<TO>` recipient and with `--team`.
    #[arg(long)]
    all: bool,
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
    /// Broadcast only: resolve and print the recipient set without sending
    /// anything. Ignored for a single-recipient send.
    #[arg(long = "dry-run")]
    dry_run: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

/// A resolved `bwoc send` invocation: one targeted recipient, or a broadcast.
enum SendMode {
    One(send::SendArgs),
    Group(send::GroupArgs),
}

impl SendArgs {
    /// Read `--file` content, trimming trailing newlines and carriage returns (a
    /// vim/EOF newline shouldn't bloat the JSONL envelope, and a Windows `\r\n`
    /// must not leave a stray `\r` in the body; explicit content stays preserved).
    fn read_file_body(path: &std::path::Path) -> Result<String, String> {
        std::fs::read_to_string(path)
            .map(|s| s.trim_end_matches(['\n', '\r']).to_string())
            .map_err(|e| format!("failed to read {}: {e}", path.display()))
    }

    /// Resolve the recipient(s) and message body into a `SendMode`. Broadcast
    /// mode (`--all` / `--team`) takes the message from whichever positional
    /// clap filled (it fills `to` first), since there is no `<TO>` recipient.
    fn resolve(self) -> Result<SendMode, String> {
        if self.all && self.team.is_some() {
            return Err("--all and --team are mutually exclusive".to_string());
        }
        let broadcast = self.all || self.team.is_some();

        if broadcast {
            if self.reply_to.is_some() {
                return Err("--reply-to has no meaning for a broadcast".to_string());
            }
            // The message is the file, else the lone positional (clap put it in
            // `to`), else the second positional. Two inline positionals with a
            // broadcast flag is ambiguous — the recipient came from the flag.
            let body = match (self.file, self.message, self.to) {
                (Some(p), None, None) => Self::read_file_body(&p)?,
                (Some(_), _, _) => {
                    return Err("--file is mutually exclusive with an inline message".to_string());
                }
                (None, Some(m), None) => m,
                (None, None, Some(t)) => t,
                (None, None, None) => {
                    return Err(
                        "broadcast needs a message: `bwoc send --all <MESSAGE>`".to_string()
                    );
                }
                (None, Some(_), Some(_)) => {
                    return Err(
                        "a broadcast takes only a message — the recipient comes from --all/--team"
                            .to_string(),
                    );
                }
            };
            let recipients = if self.all {
                send::Recipients::All
            } else {
                send::Recipients::Team(self.team.expect("checked by `broadcast`"))
            };
            return Ok(SendMode::Group(send::GroupArgs {
                recipients,
                message: body,
                from: self.from,
                no_wakeup: self.no_wakeup,
                workspace: self.workspace,
                dry_run: self.dry_run,
            }));
        }

        // Single-recipient mode.
        let to = self
            .to
            .ok_or("recipient required: pass <TO>, or --all / --team to broadcast")?;
        let body = match (self.message, self.file) {
            (Some(m), _) => m,
            (None, Some(p)) => Self::read_file_body(&p)?,
            (None, None) => return Err("message required: pass <MESSAGE> or --file".to_string()),
        };
        Ok(SendMode::One(send::SendArgs {
            to,
            message: body,
            from: self.from,
            reply_to: self.reply_to,
            no_wakeup: self.no_wakeup,
            workspace: self.workspace,
            kind: None,
            force_peer_route: false,
            require_signed: false,
        }))
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
struct LoopArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Team to open first (id). Defaults to the first team alphabetically.
    #[arg(long)]
    team: Option<String>,
    /// Ticker cadence in seconds the goal-loop shows / starts with (floored at 1s).
    #[arg(long = "interval-secs", default_value_t = 5)]
    interval_secs: u64,
    /// Iteration budget the goal-loop shows / starts with (`0` = unbounded).
    #[arg(long = "max-iters", default_value_t = 20)]
    max_iters: usize,
    /// Backend for the spawned goal-loop harness (e.g. ollama, openai-compatible,
    /// openrouter). Omit to use the harness default.
    #[arg(long)]
    backend: Option<String>,
    /// Model id for the spawned goal-loop harness. Omit to use the harness default.
    #[arg(long)]
    model: Option<String>,
    /// OpenAI-compatible endpoint for the spawned goal-loop harness. Omit to use
    /// the harness default.
    #[arg(long)]
    endpoint: Option<String>,
}

impl LoopArgs {
    fn into_runtime(self) -> loop_cmd::LoopArgs {
        loop_cmd::LoopArgs {
            workspace: self.workspace,
            team: self.team,
            ticker_secs: self.interval_secs,
            budget_iters: self.max_iters,
            backend: self.backend,
            model: self.model,
            endpoint: self.endpoint,
        }
    }
}

#[derive(Args, Debug)]
struct MonitorArgs {
    /// The shell command to probe each tick. Exit 0 = OK, non-zero = tripped
    /// (a spawn failure or signal death also counts as tripped).
    #[arg(long)]
    exec: String,
    /// Fleet member to `bwoc send` on a transition. Omit to log transitions only.
    #[arg(long)]
    alert: Option<String>,
    /// Run continuously; without it, probe once and exit (0 = OK, 1 = tripped).
    #[arg(long = "loop")]
    loop_mode: bool,
    /// Cadence between probes in `--loop` mode (seconds, floored at 1s).
    #[arg(long = "interval-secs", default_value_t = 60)]
    interval_secs: u64,
    /// Iteration budget in `--loop` mode. `0` = unbounded (a monitor is a service
    /// — stop it with Ctrl-C / a supervisor).
    #[arg(long = "max-iters", default_value_t = 0)]
    max_iters: usize,
    /// Stable monitor id (ledger key + `.bwoc/monitors/<id>.jsonl`). Omit to
    /// derive it from `--exec` so the monitor keeps its state across restarts.
    #[arg(long)]
    id: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

impl MonitorArgs {
    fn into_runtime(self) -> monitor::MonitorArgs {
        monitor::MonitorArgs {
            exec: self.exec,
            alert: self.alert,
            loop_mode: self.loop_mode,
            interval_secs: self.interval_secs,
            max_iters: self.max_iters,
            id: self.id,
            workspace: self.workspace,
        }
    }
}

/// clap value-parser for `--period` — rejects an unknown period with a helpful
/// message instead of silently defaulting.
fn parse_period(s: &str) -> Result<digest::Period, String> {
    digest::Period::parse(s)
        .ok_or_else(|| format!("invalid period '{s}' (use hourly | daily | weekly)"))
}

#[derive(Args, Debug)]
struct DigestArgs {
    /// The command whose output is the digest content (operator-defined).
    #[arg(long)]
    exec: String,
    /// How often to deliver. One of: hourly | daily | weekly.
    #[arg(long, default_value = "daily", value_parser = parse_period)]
    period: digest::Period,
    /// Write the digest to this file (appended) instead of stdout. Either sink is
    /// local — v1 never delivers into a fleet agent's model-read inbox.
    #[arg(long = "out")]
    out: Option<PathBuf>,
    /// Run continuously; without it, deliver at most once for the current period
    /// and exit (the cron-driven mode).
    #[arg(long = "loop")]
    loop_mode: bool,
    /// Poll cadence in `--loop` mode (seconds, floored at 1s) — how often to check
    /// whether a new period has begun.
    #[arg(long = "interval-secs", default_value_t = 300)]
    interval_secs: u64,
    /// Iteration budget in `--loop` mode. `0` = unbounded (a service).
    #[arg(long = "max-iters", default_value_t = 0)]
    max_iters: usize,
    /// Stable digest id (ledger + `.bwoc/digests/<id>.jsonl`). Omit to derive it
    /// from `--exec` + `--period`.
    #[arg(long)]
    id: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
}

impl DigestArgs {
    fn into_runtime(self) -> digest::DigestArgs {
        digest::DigestArgs {
            exec: self.exec,
            period: self.period,
            out: self.out,
            loop_mode: self.loop_mode,
            interval_secs: self.interval_secs,
            max_iters: self.max_iters,
            id: self.id,
            workspace: self.workspace,
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
struct HandbookCliArgs {
    /// Section to print (start, agents, spawn, teams, harness, release).
    /// Omit to list all sections.
    section: Option<String>,
}

#[derive(Args, Debug)]
struct InfoCliArgs {
    /// Workspace root (defaults to BWOC_WORKSPACE or the ancestor walk from cwd).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Emit machine-readable JSON instead of the human card.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ReportCliArgs {
    /// Issue title (required).
    title: Option<String>,
    /// Issue body; an Environment block (version/release/OS) is appended.
    #[arg(long)]
    body: Option<String>,
    /// Report kind → GitHub label (feature maps to `enhancement`).
    #[arg(long, default_value = "bug", value_parser = ["bug", "feature", "question"])]
    kind: String,
    /// Print a prefilled browser URL instead of creating via `gh`.
    #[arg(long)]
    web: bool,
    /// Skip the confirmation prompt.
    #[arg(long)]
    yes: bool,
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
struct EvalCliArgs {
    /// The fixture directory (holds `fixture.toml` + optional `seed/` / `expected/`).
    fixture: PathBuf,
    /// Provider backend forwarded to the harness (`ollama` / `openai-compatible` /
    /// `claude` / `openrouter` / `cli`). Omitted ⇒ the harness default.
    #[arg(long)]
    backend: Option<String>,
    /// Model id forwarded to the harness.
    #[arg(long)]
    model: Option<String>,
    /// OpenAI-compatible endpoint base URL forwarded to the harness.
    #[arg(long)]
    endpoint: Option<String>,
    /// Work dir to seed the fixture into. Default: a fresh temp dir (isolated).
    #[arg(long)]
    workdir: Option<PathBuf>,
    /// Emit the scored result as JSON instead of a human report.
    #[arg(long)]
    json: bool,
}

impl From<EvalCliArgs> for eval::EvalArgs {
    fn from(a: EvalCliArgs) -> Self {
        Self {
            fixture: a.fixture,
            backend: a.backend,
            model: a.model,
            endpoint: a.endpoint,
            workdir: a.workdir,
            json: a.json,
        }
    }
}

#[derive(Args, Debug)]
struct TasksCliArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Show only tasks claimed by this agent (id or bare name).
    #[arg(long)]
    agent: Option<String>,
    /// Show only tasks in this state: `pending` | `in_progress` | `completed`.
    #[arg(long)]
    state: Option<String>,
    /// Emit a machine-readable JSON object instead of the human table.
    #[arg(long)]
    json: bool,
}

impl From<TasksCliArgs> for tasks::TasksArgs {
    fn from(a: TasksCliArgs) -> Self {
        Self {
            workspace: a.workspace,
            agent: a.agent,
            state: a.state,
            json: a.json,
        }
    }
}

#[derive(Args, Debug)]
struct ReceiptsCliArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Show only the receipt for this source message id (the `bwoc send` `[id …]`).
    #[arg(long = "message-id")]
    message_id: Option<String>,
    /// Show only receipts for messages from this sender (e.g. `user`, `agent-x`).
    #[arg(long)]
    from: Option<String>,
    /// Show only receipts recorded by this recipient agent (id or bare name).
    #[arg(long)]
    agent: Option<String>,
    /// Emit a machine-readable JSON object instead of the human table.
    #[arg(long)]
    json: bool,
}

impl From<ReceiptsCliArgs> for receipts::ReceiptsArgs {
    fn from(a: ReceiptsCliArgs) -> Self {
        Self {
            workspace: a.workspace,
            message_id: a.message_id,
            from: a.from,
            agent: a.agent,
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
    /// Emit JSON `{ workspace, name, version, defaults, runtime, profile,
    /// files_created }` instead of the human-readable creation report.
    #[arg(long)]
    json: bool,
    /// Scaffold without agent runtime/daemon provisioning — for CI,
    /// read-only, or inspection workspaces that never spawn agents. The
    /// workspace is still valid (`bwoc check` passes); the daemon-ephemeral
    /// `.gitignore` patterns are simply omitted.
    #[arg(long = "no-runtime")]
    no_runtime: bool,
    /// Initialize a single-agent workspace (one agent slot) instead of the
    /// multi-agent fleet default. Scaffolds single-agent-oriented guidance.
    #[arg(long = "single-agent")]
    single_agent: bool,
}

impl InitArgs {
    fn into_runtime(self, lang: String) -> init::InitArgs {
        init::InitArgs {
            path: self.path,
            force: self.force,
            lang,
            json: self.json,
            no_runtime: self.no_runtime,
            single_agent: self.single_agent,
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
    /// Workspace the agent lands in when --target is not given. Standard
    /// resolution: this flag > BWOC_WORKSPACE env > ancestor walk from cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Target directory for the new agent. Default:
    /// <workspace>/<agents_dir>/agent-<name> when a workspace resolves;
    /// otherwise next to the template (fresh clone) or under cwd.
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
    /// Derive the agent bound to a base project (the "debase" relationship):
    /// sets worktreeBase to <project>/worktrees and seeds the build/test/lint/
    /// format gates from the project's detected stack. Explicit gate flags win.
    #[arg(long)]
    project: Option<PathBuf>,
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
    /// Non-interactive: never prompt. The four gate commands fall back to their
    /// stack-detected defaults (or `true` when unknown), so a fleet can be
    /// scaffolded from a script with just `--role` + `--primary-model`.
    #[arg(long, visible_alias = "non-interactive")]
    yes: bool,
}

impl NewArgs {
    fn into_runtime(self, lang: String) -> new::NewArgs {
        new::NewArgs {
            name: self.name,
            target: self.target,
            workspace: self.workspace,
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
            project: self.project,
            scope: self.scope,
            out_of_scope: self.out_of_scope,
            primary_capability: self.primary_capability,
            mindsets: self.mindsets,
            skills: self.skills,
            json: self.json,
            yes: self.yes,
        }
    }
}

#[derive(Args, Debug)]
struct DebaseArgs {
    #[command(subcommand)]
    command: DebaseCommand,
    /// Workspace root. Default: --workspace > BWOC_WORKSPACE > ancestor walk.
    #[arg(long = "workspace", global = true)]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human-readable output.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(clap::Subcommand, Debug)]
enum DebaseCommand {
    /// List every agent → its base project + buildable stack.
    List,
    /// Show one agent's binding detail.
    Show {
        /// Agent name (with or without the `agent-` prefix).
        agent: String,
    },
    /// (Re)bind an agent to a base project — sets worktreeBase to
    /// `<project>/worktrees`.
    Set {
        /// Agent name (with or without the `agent-` prefix).
        agent: String,
        /// Base project directory (must exist; canonicalized to absolute).
        project: PathBuf,
        /// Skip the TTY confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

impl DebaseArgs {
    fn into_runtime(self) -> debase::DebaseArgs {
        let action = match self.command {
            DebaseCommand::List => debase::DebaseAction::List,
            DebaseCommand::Show { agent } => debase::DebaseAction::Show { agent },
            DebaseCommand::Set {
                agent,
                project,
                yes,
            } => debase::DebaseAction::Set {
                agent,
                project,
                yes,
            },
        };
        debase::DebaseArgs {
            action,
            workspace: self.workspace,
            json: self.json,
        }
    }
}

#[derive(Args, Debug)]
struct RemoteArgs {
    #[command(subcommand)]
    command: RemoteCommand,
    /// Workspace root. Default: --workspace > BWOC_WORKSPACE > ancestor walk.
    #[arg(long = "workspace", global = true)]
    workspace: Option<PathBuf>,
    /// Emit JSON instead of the human-readable output.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(clap::Subcommand, Debug)]
enum RemoteCommand {
    /// Link an agent to a remote-control session (records bwoc-side bookkeeping).
    Link {
        /// Agent name (with or without the `agent-` prefix).
        agent: String,
        /// Opaque reference to the external session (id / token / handle).
        session_ref: String,
        /// Backend that exposes the session. Default: the agent's manifest backend.
        #[arg(long)]
        backend: Option<String>,
        /// Remote-control mechanism. Default: `claude-remote-control`.
        #[arg(long)]
        kind: Option<String>,
        /// Optional control URL for the session.
        #[arg(long)]
        url: Option<String>,
        /// Optional free-text note.
        #[arg(long)]
        note: Option<String>,
    },
    /// List every recorded remote-control link.
    List,
    /// Show one agent's remote-control link.
    Status {
        /// Agent name (with or without the `agent-` prefix).
        agent: String,
    },
    /// Remove an agent's remote-control link.
    Unlink {
        /// Agent name (with or without the `agent-` prefix).
        agent: String,
        /// Skip the TTY confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

impl RemoteArgs {
    fn into_runtime(self) -> remote::RemoteArgs {
        let action = match self.command {
            RemoteCommand::Link {
                agent,
                session_ref,
                backend,
                kind,
                url,
                note,
            } => remote::RemoteAction::Link {
                agent,
                session_ref,
                backend,
                kind,
                url,
                note,
            },
            RemoteCommand::List => remote::RemoteAction::List,
            RemoteCommand::Status { agent } => remote::RemoteAction::Status { agent },
            RemoteCommand::Unlink { agent, yes } => remote::RemoteAction::Unlink { agent, yes },
        };
        remote::RemoteArgs {
            action,
            workspace: self.workspace,
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
        Some(Commands::Eval(args)) => {
            let code = eval::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Retire(args)) => {
            let code = retire::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Set {
            name,
            backend,
            primary_model,
            fallback_model,
            workspace,
            json,
        }) => {
            let code = set::run(set::SetArgs {
                name,
                backend,
                primary_model,
                fallback_model,
                workspace,
                json,
            });
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Debase(args)) => {
            let code = debase::run(args.into_runtime());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Remote(args)) => {
            let code = remote::run(args.into_runtime());
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
        Some(Commands::Handbook(args)) => {
            let code = handbook::run(handbook::HandbookArgs {
                section: args.section,
                lang: lang.clone(),
            });
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Info(args)) => {
            let code = info::run(info::InfoArgs {
                workspace: args.workspace,
                json: args.json,
            });
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Report(args)) => {
            let code = report::run(report::ReportArgs {
                title: args.title,
                body: args.body,
                kind: args.kind,
                web: args.web,
                yes: args.yes,
            });
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
        Some(Commands::Loop(args)) => {
            let code = loop_cmd::run(args.into_runtime());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Monitor(args)) => {
            let code = monitor::run(args.into_runtime());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Digest(args)) => {
            let code = digest::run(args.into_runtime());
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
            let code = match args.resolve() {
                Ok(SendMode::One(runtime)) => send::run(runtime),
                Ok(SendMode::Group(runtime)) => send::run_group(runtime),
                Err(e) => {
                    eprintln!("bwoc send: {e}");
                    return ExitCode::from(2);
                }
            };
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
        Some(Commands::Agent(sub)) => {
            let code = match sub {
                AgentCommand::Run(args) => agent_run::run(args.into()),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Inbox(args)) => {
            let code = inbox::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Triage(args)) => {
            let code = triage::run(args.into());
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
        Some(Commands::Tasks(args)) => {
            let code = tasks::run(args.into());
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Receipts(args)) => {
            let code = receipts::run(args.into());
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
            let code = match sub.command {
                // Bare `bwoc fleet` → the status overview (issue #297).
                None => fleet::status(fleet::FleetStatusArgs {
                    workspace: None,
                    json: false,
                }),
                Some(FleetCommand::Status(args)) => fleet::status(fleet::FleetStatusArgs {
                    workspace: args.workspace,
                    json: args.json,
                }),
                Some(FleetCommand::Health(args)) => fleet::run(fleet::FleetHealthArgs {
                    workspace: args.workspace,
                    json: args.json,
                    stale_days: args.stale_days,
                    loop_mode: args.loop_mode,
                    loop_interval_secs: args.loop_interval_secs,
                    loop_max_iters: args.loop_max_iters,
                }),
                Some(FleetCommand::Term(args)) => fleet_term::run(fleet_term::FleetTermArgs {
                    workspace: args.workspace,
                    layout: args.layout,
                    session: args.session,
                    print: args.print,
                }),
            };
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Outbox(args)) => {
            let cmd = match args.command {
                None | Some(OutboxCommand::List) => outbox_cmd::OutboxCmd::List,
                Some(OutboxCommand::Flush { peer }) => outbox_cmd::OutboxCmd::Flush { peer },
            };
            let code = outbox_cmd::run(outbox_cmd::OutboxArgs {
                cmd,
                workspace: args.workspace,
            });
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
        Some(Commands::Gcloud(sub)) => {
            // gcloud mirrors jira's dispatch — own arg structs in gcloud.rs;
            // exit-code convention there (0/1/2/4/255).
            let code = gcloud::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Okr(sub)) => {
            // okr mirrors gcloud's dispatch — own arg structs in okr.rs;
            // exit-code convention there (0/1/2/4/255).
            let code = okr::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Council(sub)) => {
            // council owns its arg structs in council.rs; exit-code convention
            // there (0/1/2/3/4/255 — 3 = resolve reached quorum but no outcome).
            let code = council::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Figma(sub)) => {
            // figma mirrors okr's dispatch — own arg structs in figma.rs;
            // exit-code convention there (0/1/2/4/255).
            let code = figma::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Gws(sub)) => {
            // gws mirrors gcloud's dispatch — own arg structs in gws.rs;
            // exit-code convention there (0/1/2/4/255).
            let code = gws::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Accounting(sub)) => {
            // accounting mirrors gcloud/gws — own arg structs in accounting.rs;
            // same exit-code convention (0/1/2/4/255).
            let code = accounting::run(sub);
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Some(Commands::Resource(sub)) => {
            let code = resource::run(sub);
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
                    allow_unauthenticated: args.allow_unauthenticated,
                }),
                A2aCommand::FetchCard(args) => a2a::run_fetch_card(a2a::FetchCardArgs {
                    url: args.url,
                    token: args.token,
                    workspace: args.workspace,
                }),
                A2aCommand::Send(args) => a2a::run_send_outbound(a2a::SendOutboundArgs {
                    url: args.url,
                    message: args.message,
                    context: args.context,
                    token: args.token,
                    workspace: args.workspace,
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

#[cfg(test)]
mod send_resolve_tests {
    use super::*;

    /// Build a `SendArgs` with everything empty; tests set only the fields they
    /// exercise so the disambiguation logic is the subject.
    fn args() -> SendArgs {
        SendArgs {
            to: None,
            message: None,
            file: None,
            team: None,
            all: false,
            from: None,
            reply_to: None,
            no_wakeup: false,
            dry_run: false,
            workspace: None,
        }
    }

    #[test]
    fn single_recipient_with_message_resolves_to_one() {
        let a = SendArgs {
            to: Some("agent-x".into()),
            message: Some("hello".into()),
            ..args()
        };
        match a.resolve().unwrap() {
            SendMode::One(s) => {
                assert_eq!(s.to, "agent-x");
                assert_eq!(s.message, "hello");
            }
            SendMode::Group(_) => panic!("expected single"),
        }
    }

    #[test]
    fn broadcast_all_takes_message_from_the_to_positional() {
        // `bwoc send --all "hi"` — clap puts "hi" in `to` (positional 1).
        let a = SendArgs {
            all: true,
            to: Some("hi".into()),
            ..args()
        };
        match a.resolve().unwrap() {
            SendMode::Group(g) => {
                assert!(matches!(g.recipients, send::Recipients::All));
                assert_eq!(g.message, "hi");
            }
            SendMode::One(_) => panic!("expected group"),
        }
    }

    #[test]
    fn broadcast_team_names_the_team() {
        let a = SendArgs {
            team: Some("tianting".into()),
            to: Some("go".into()),
            ..args()
        };
        match a.resolve().unwrap() {
            SendMode::Group(g) => match g.recipients {
                send::Recipients::Team(t) => assert_eq!(t, "tianting"),
                send::Recipients::All => panic!("expected team"),
            },
            SendMode::One(_) => panic!("expected group"),
        }
    }

    #[test]
    fn all_and_team_together_is_error() {
        let a = SendArgs {
            all: true,
            team: Some("t".into()),
            to: Some("m".into()),
            ..args()
        };
        assert!(a.resolve().is_err());
    }

    #[test]
    fn broadcast_without_a_message_is_error() {
        let a = SendArgs {
            all: true,
            ..args()
        };
        assert!(a.resolve().is_err());
    }

    #[test]
    fn broadcast_with_two_positionals_is_ambiguous_error() {
        // recipient must come from the flag, not a positional.
        let a = SendArgs {
            all: true,
            to: Some("agent-x".into()),
            message: Some("hi".into()),
            ..args()
        };
        assert!(a.resolve().is_err());
    }

    #[test]
    fn broadcast_rejects_reply_to() {
        let a = SendArgs {
            all: true,
            to: Some("hi".into()),
            reply_to: Some("msg-1".into()),
            ..args()
        };
        assert!(a.resolve().is_err());
    }

    #[test]
    fn single_without_recipient_is_error() {
        let a = SendArgs {
            message: Some("hi".into()),
            ..args()
        };
        assert!(a.resolve().is_err());
    }
}
