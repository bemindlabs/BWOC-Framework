//! `bwoc help [topic]` — topic-specific help embedded in the binary.
//!
//! `--help` (clap-generated) covers per-flag mechanics. This complements
//! it: topical guides for concepts that span multiple commands —
//! backends, workspace layout, manifest fields, the arc, and a quick
//! getting-started cheatsheet.
//!
//! `bwoc help` with no topic lists what's available.

use std::io::{self, IsTerminal};

pub struct HelpArgs {
    pub topic: Option<String>,
    /// Print every topic concatenated (with separators). Useful for
    /// offline reading or piping into a docs generator. Mutually
    /// exclusive with `<topic>`.
    pub all: bool,
}

const TOPICS: &[(&str, &str, &str)] = &[
    (
        "getting-started",
        "5-step quickstart from `bwoc init` to first spawn",
        GETTING_STARTED,
    ),
    (
        "backends",
        "The 6 declared backends and how to switch between them",
        BACKENDS,
    ),
    (
        "workspace",
        "Workspace layout: .bwoc/, agents/, projects/, notes/",
        WORKSPACE,
    ),
    ("manifest", "config.manifest.json field reference", MANIFEST),
    (
        "arc",
        "uppāda · ṭhiti · vaya — how commands map to the agent lifecycle",
        ARC,
    ),
    (
        "lifecycle",
        "Agent state machine: new · start · stop · retire (registry + daemon)",
        LIFECYCLE,
    ),
    (
        "daemon",
        "bwoc-agent --serve internals: PID, socket, inbox cursor, doctor sweeps",
        DAEMON,
    ),
    (
        "messaging",
        "Inbox flow: send · inbox · --watch · --clear (sammā-vācā Phase 0)",
        MESSAGING,
    ),
    (
        "persona",
        "Per-agent identity: scope, out-of-scope, mindsets, skills",
        PERSONA,
    ),
    (
        "memory",
        "Workspace-level memory at .bwoc/memory/: the per-workspace tier",
        MEMORY,
    ),
    (
        "doctor",
        "Env + workspace diagnostic with auto-fix sweeps and JSON output",
        DOCTOR,
    ),
    (
        "script",
        "Shell-script idioms: --count, --names-only, --json, --path-only",
        SCRIPT,
    ),
];

pub fn run(args: HelpArgs) -> i32 {
    let topic = args.topic.as_deref();
    let colored = io::stdout().is_terminal();
    let c = Colors::for_tty(colored);

    // --all: print every topic concatenated. Skips the index/colors for
    // grep-ability and pipe-friendliness. Separators between topics so
    // a docs builder can split on the `# === <name> ===` line.
    if args.all {
        for (name, summary, body) in TOPICS {
            println!();
            println!("# === {name} ===");
            println!("# {summary}");
            println!();
            print!("{body}");
            println!();
        }
        return 0;
    }

    let Some(name) = topic else {
        print_index(&c);
        return 0;
    };

    let Some((_, _, body)) = TOPICS.iter().find(|(t, _, _)| *t == name) else {
        eprintln!("bwoc help: unknown topic '{name}'");
        eprintln!();
        print_index(&c);
        return 2;
    };

    println!();
    println!("{}# {name}{}", c.bold_cyan, c.reset);
    println!();
    print!("{body}");
    println!();
    0
}

fn print_index(c: &Colors) {
    println!();
    println!("{}Available help topics:{}", c.bold_cyan, c.reset);
    println!();
    for (name, summary, _) in TOPICS {
        println!("  {}{name:<20}{}  {summary}", c.yellow, c.reset);
    }
    println!();
    println!(
        "{}Usage:{} bwoc help <topic>     (e.g. `bwoc help getting-started`)",
        c.dim, c.reset
    );
    println!(
        "{}      {} bwoc --help            for per-command flags",
        c.dim, c.reset
    );
    println!();
    println!(
        "{}Online docs:{} https://bemindlabs.github.io/BWOC-Framework/",
        c.dim, c.reset
    );
    println!();
}

struct Colors {
    bold_cyan: &'static str,
    yellow: &'static str,
    dim: &'static str,
    reset: &'static str,
}

impl Colors {
    fn for_tty(on: bool) -> Self {
        if on {
            Self {
                bold_cyan: "\x1b[1;36m",
                yellow: "\x1b[33m",
                dim: "\x1b[2m",
                reset: "\x1b[0m",
            }
        } else {
            Self {
                bold_cyan: "",
                yellow: "",
                dim: "",
                reset: "",
            }
        }
    }
}

// --- topic content (markdown-flavored plain text) -------------------------

const GETTING_STARTED: &str = "\
1. Initialize a workspace
   $ mkdir my-workspace && cd my-workspace
   $ bwoc init

2. Incarnate your first agent (interactive — press Enter to accept defaults)
   $ bwoc new alpha
     agentRole       picker (default: code reviewer)
     primaryModel    picker per --backend (default: first model in catalog)
     lint/format/test/build cmds  stack-detected defaults

3. Verify backend neutrality
   $ bwoc check agents/agent-alpha

4. See what's registered + health
   $ bwoc list
   $ bwoc status

5. Spawn the configured backend in the agent's directory
   $ bwoc spawn --path agents/agent-alpha --backend claude

See also:
  bwoc help workspace    — what each directory means
  bwoc help backends     — switching between claude/agy/codex/kimi/ollama
  bwoc help arc          — uppāda · ṭhiti · vaya mapping
  examples/howto/        — full walkthroughs
";

const BACKENDS: &str = "\
BWOC supports 6 declared backends (Samānattatā — equal treatment, no lock-in):

  | Backend     | CLI binary    | Common models                                          |
  |-------------|---------------|--------------------------------------------------------|
  | Claude      | claude        | claude-opus-4-8, claude-sonnet-4-6, claude-haiku-4-5   |
  | Antigravity | agy           | gemini-3.5-flash-*, gemini-3.1-pro-*, claude-*-thinking, gpt-oss-120b-* |
  | Codex       | codex         | gpt-5.5, gpt-5.4, gpt-5.4-mini, gpt-5.3-codex          |
  | Kimi        | kimi          | kimi-k2, kimi-k1.5                                     |
  | Ollama      | bwoc-harness  | qwen2.5-coder:7b, llama3.1:8b, mistral-nemo, gemma4:8b |
  | OpenAI-compatible | bwoc-harness | gpt-5.5, gpt-5.5-pro, gpt-5.4, gpt-5.4-mini       |

Each agent picks ONE backend at incarnation, recorded in its
config.manifest.json (primaryModel + optional fallbackModel) and in
.bwoc/agents.toml.

All 6 read the SAME AGENTS.md via symlinks (CLAUDE.md / AGY.md /
CODEX.md / KIMI.md / OLLAMA.md / OPENAI.md all → AGENTS.md). If your agent's
instructions assume a specific backend, `bwoc check` flags it as a
neutrality violation.

The Ollama backend differs from the others: instead of exec'ing an external
vendor CLI, `bwoc spawn --backend ollama` launches the `bwoc-harness` sibling
binary, which drives the agentic loop against any OpenAI-compatible endpoint
(default: http://localhost:11434/v1). Run `bwoc doctor` to check whether
`bwoc-harness` is available and whether Ollama is reachable.

Three ways to set the backend:
  - At incarnation:   bwoc new my-agent --backend ollama
  - Manifest edit:    edit agents/<name>/config.manifest.json then update
                      .bwoc/agents.toml's `backend = \"...\"`
  - Per spawn:        bwoc spawn --path agents/<name> --backend ollama
                      (overrides for one session — useful for cross-
                       backend testing)

See: bwoc help manifest  — full manifest field reference
     examples/howto/configure-backends.md
";

const WORKSPACE: &str = "\
A BWOC workspace is any directory containing `.bwoc/workspace.toml`.

After `bwoc init`:

  my-workspace/
  ├── .bwoc/                  workspace config (do not delete)
  │   ├── workspace.toml      name, version, defaults
  │   └── agents.toml         registry of incarnated agents
  ├── agents/                 incarnated agents land here (per agents_dir)
  │   └── README.md           what agents/ is for
  ├── projects/               your work — apps, repos, libraries
  │   └── README.md           convention reminder
  └── notes/                  YYYY-MM-DD_<title>.md implementation logs
      └── README.md           naming convention

What's enforced (CLI requires):
  - .bwoc/workspace.toml      bwoc init creates; bwoc workspace validate checks
  - .bwoc/agents.toml         bwoc new appends; bwoc retire removes
  - agents/ or defaults.agents_dir  where new agents land

What's suggested (auto-created by bwoc init, you can rm or customize):
  - projects/                 your work
  - notes/                    implementation logs
  - README.md in each scaffold dir

What the CLI never touches:
  - Anything outside .bwoc/ and agents/ — your projects/ and notes/
    are yours
  - Files in an agent dir after incarnation — once bwoc new finishes,
    the agent's tree is yours to edit

Central per-user state (independent of any workspace):
  ~/.bwoc/
  ├── config.toml             per-user config (yours to edit)
  ├── template/               optional — extracted agent template cache
  └── memory/                 central per-user memory (Phase 2+)

Workspace resolution chain (for any cmd that needs to find one):
  --workspace <path>          explicit
  BWOC_WORKSPACE              env var
  ancestor walk for .bwoc/    from cwd upward
  cwd self-check              last resort
  exit 2 with hint            if none found

Validate the layout:
  bwoc workspace validate     — 5 spec rules (exit 0 clean, 2 violations)
  bwoc doctor                 — richer; with --auto, fixes safe issues
  bwoc workspace prune        — find drift between registry and disk
";

const MANIFEST: &str = "\
Every incarnated agent has `config.manifest.json` at its root.
Schema (resolved by `bwoc new`, written verbatim — no placeholders):

  Required (prompted if missing):
    name              kebab-case agent name (e.g. \"alpha\")
    agentId           always \"agent-{name}\" (auto-derived)
    agentRole         one-or-two-word role description
    primaryModel      LLM model identifier (e.g. \"claude-opus-4-8\")
                      or \"auto\" for harness-backed runtime selection
    lintCmd           shell command for lint
    formatCmd         shell command for format check
    testCmd           shell command for tests
    buildCmd          shell command for build
    memoryPath        relative path to per-agent memory dir
                      (default: \"memories/\")
    version           manifest schema version (auto: \"2.0\")

  Optional (no prompt; pass --flag if you want them):
    fallbackModel     fallback LLM model identifier
    autoModels        ordered candidate pool used when primaryModel=\"auto\"
    reasoningEffort   optional backend effort control (e.g. \"medium\")
    sessionsPath      session data dir for Tier 2 memory mining
    deepMemoryCmd     Tier 2 memory CLI command
    worktreeBase      base path for spawned worktrees

  Auto-managed (not prompted, not exposed as flag):
    requiredConfig    field descriptions (copied from the template
                      manifest so `bwoc new` can use them for prompts)

Where the values come from:
  - --flag                          highest precedence
  - interactive picker / default    if TTY and flag missing
  - non-TTY without flag            fail-fast with MissingFields error

See: bwoc new --help    — every flag listed
     docs/en/INCARNATION.en.md §\"Setting the Manifest\"
";

const ARC: &str = "\
BWOC models an agent's life as three phases (AN 3.47 Saṅkhata Sutta):

  uppāda    arising      — an agent is created
  ṭhiti     persisting   — an agent operates
  vaya      ceasing      — an agent ends

The CLI maps directly:

  Phase 1 v2.0 (uppāda, shipped):
    bwoc init <path>          establish the workspace
    bwoc new <name>            incarnate an agent
    bwoc check <path>          audit backend neutrality
    bwoc spawn --path <p>      bring the agent to life (exec the backend)

  Phase 2 (ṭhiti, in progress):
    bwoc status [name]         per-agent health snapshot (shipped)
    bwoc list                  registry view (shipped)
    bwoc workspace info        workspace overview (shipped)
    bwoc workspace validate    enforce layout rules (shipped)
    bwoc workspace prune       reconcile registry vs disk (shipped)
    bwoc doctor                env + workspace diagnostic (shipped)
    --- not yet ---
    bwoc-agent control socket  long-running process supervision
    bwoc log / send / stop     interact with a live agent

  Phase 3 (vaya, partially shipped):
    bwoc retire <name>         remove from registry (+ optional files) — shipped
    --- not yet ---
    bwoc stop <name>           graceful signal-escalation shutdown
    full vaya cleanup          worktree, branch, memory, interconnect

  Phase 4 (adoption — beyond code):
    Reference agents, fleet patterns, ecosystem growth.

Every command's purpose, in one phrase: \"which phase of the arc
does this push the agent into?\". Commands that don't fit are
infrastructure (init, doctor) — they configure the environment,
not the agent.

See: docs/en/ROADMAP.en.md         — full phase definitions
     docs/en/PHILOSOPHY.en.md §0.1  — the arc in detail
";

const LIFECYCLE: &str = "\
Agent state machine — registry intent + daemon process state.

Verbs:
  bwoc new <name>      Create + register a new agent (uppāda).
                       Interactive picker for backend/role/model;
                       writes config.manifest.json + appends to
                       .bwoc/agents.toml.

  bwoc start <name>    Idempotent — runs both side effects as needed:
                         status = active in registry
                         spawn `bwoc-agent --serve` if no daemon
                       Use --no-daemon to skip the spawn (registry
                       flip only).

  bwoc stop <name>     Idempotent — runs both side effects as needed:
                         send STOP over .bwoc/agent.sock if daemon alive
                         status = stopped in registry
                       Files stay on disk.

  bwoc retire <name>   Remove from registry (vaya). With --keep-files
                       leaves the agent dir; without, deletes it too.

State matrix (all combinations are valid):
  status   | daemon  | typical command
  ---------|---------|----------------
  active   | running | normal operating state
  active   | none    | crashed or --no-daemon; `bwoc start` to spawn
  stopped  | none    | paused; `bwoc start` to resume
  stopped  | running | brief — `bwoc stop` is mid-cleanup

Registry intent vs runtime state:
  - `bwoc list` shows both: ● (daemon alive) / ○ (not), plus
    STATUS column for registry value.
  - `bwoc status <name>` shows the runtime line including
    `● running (pid N, uptime 5m12s)` when the socket answers
    STATUS.
  - `bwoc doctor` sweeps stale debris from crashes (PID files,
    sockets, inbox cursors).

See: bwoc help daemon      — what bwoc-agent --serve actually does
     bwoc help messaging   — inbox flow once the daemon is running
";

const DAEMON: &str = "\
`bwoc-agent --serve` — the per-agent daemon. Long-running process
inside an agent's directory; the IPC server backing `bwoc ping` /
`bwoc status`'s uptime line / `bwoc stop`'s socket signal.

Files it owns under <agent>/.bwoc/:
  agent.pid          Decimal PID of the daemon process. Used by
                     `bwoc list` / `bwoc status` for the ●/○ liveness
                     indicator (PID file + signal-0 check).
  agent.sock         Unix domain socket. Accepts the IPC protocol.
  inbox.cursor       Byte offset into inbox.jsonl marking what the
                     daemon has already consumed. Persists across
                     restarts so a daemon offline period doesn't
                     skip messages.

Files it READS (created by other commands):
  config.manifest.json   Agent identity + role + model + commands.
  inbox.jsonl            Append-only message log. The daemon polls
                         this every ~100ms and announces new envelopes
                         to its stderr.

IPC protocol (line-text; debuggable with `nc -U`):
  PING\\n      → PONG\\n
  STATUS\\n    → OK uptime_secs=<N> pid=<N>\\n
  STOP\\n      → OK shutting down\\n  (daemon exits cleanly)
  *\\n         → ERR unknown command\\n

Lifecycle:
  $ bwoc start alpha             # spawns bwoc-agent --serve
  $ bwoc status alpha            # runtime: ● running (pid N, uptime Xs)
  $ bwoc ping alpha              # alpha → PONG
  $ bwoc stop alpha              # signals daemon STOP

Direct invocation (less common):
  $ cd agents/agent-alpha
  $ bwoc-agent --serve           # blocks until SIGTERM/SIGINT
                                 # cleans up .pid + .sock on graceful exit

Crashes & cleanup:
  Graceful exit removes .pid and .sock. Hard kills (SIGKILL, OOM)
  leave them behind — `bwoc doctor --auto` sweeps both:
    bwoc doctor --auto
      FIXED  agent pid: alpha — removed stale PID file
      FIXED  agent sock: alpha — removed stale socket
      FIXED  inbox cursor: alpha — removed out-of-bounds cursor

Restart-on-crash supervision:
  `bwoc start` spawns the daemon and forgets it. For auto-respawn
  on crash, run `bwoc supervise <agent>` instead — that command
  blocks waiting on the daemon, restarts it on non-zero exit, and
  exits 0 when the daemon stops cleanly (e.g. via `bwoc stop`).
  Rate-limit guard: 10 restarts/min by default (`--max-restarts-per-min N`).
  Usage pattern:
    tmux new-window 'bwoc supervise alpha'        # interactive
    bwoc supervise alpha                          # inside a systemd unit

See: bwoc help messaging  — inbox flow (the daemon's main work)
     bwoc help lifecycle  — when to start/stop/retire
";

const MESSAGING: &str = "\
sammā-vācā Phase 0 — user → agent inbox communication. Append-only
JSON-lines file at <agent>/.bwoc/inbox.jsonl. Future phases add
agent → agent SEND with trust scoring (Kalyāṇamitta 7).

Envelope shape (one JSON object per line):
  {\"ts\": \"<ISO 8601 UTC>\",
   \"from\": \"user\",
   \"to\": \"<agent-id>\",
   \"message\": \"...\"}

Commands:

  bwoc send <to> <msg>           Append a message to the agent's inbox.
                                 No daemon required — the file gets
                                 created on first send.

  bwoc inbox <agent>             Read all messages (or `--limit N` for
                                 just the last N, `--json` for machine
                                 output).

  bwoc inbox <agent> --watch     Tail mode — block printing new
                                 envelopes as they arrive (Ctrl-C to
                                 stop). Pairs with `bwoc send` in
                                 another terminal for interactive use.

  bwoc inbox <agent> --clear     Acknowledge / truncate the inbox after
                                 reading. TTY prompts unless --yes.
                                 The daemon notices the truncation on
                                 its next 100ms poll and resets cursor.

Daemon-side behavior (when bwoc-agent --serve is running):
  - Watches inbox.jsonl every ~100ms
  - Tracks consumed offset in .bwoc/inbox.cursor (persists across
    daemon restarts)
  - Announces each new envelope to stderr:
      bwoc-agent: inbox ← user: <message>

Interactive workflow (typical):
  Terminal A:  bwoc start alpha            # daemon up
  Terminal A:  bwoc inbox alpha --watch    # live view
  Terminal B:  bwoc send alpha \"do thing\"  # arrives in <300ms

`bwoc list` shows the INBOX column with each agent's pending count
(rendered as \"—\" for empty inboxes).

See: bwoc help daemon     — what reads inbox.jsonl on the daemon side
     bwoc help lifecycle  — the state machine inbox commands work with
";

const PERSONA: &str = "\
Per-agent identity — WHO the agent is, WHAT it does, HOW it thinks,
what it KNOWS. Four slots live inside each incarnated agent's tree:

  persona/README.md   identity + role + scope (single file)
  mindsets/<*>.md     decision-making frameworks (one per file)
  skills/<*>.md       concrete capabilities (one per file)
  memories/<*>.md     accumulated knowledge (one per file)

Configure these at incarnation time via `bwoc new` flags:

  bwoc new <name> \\
    --role 'code reviewer' \\
    --scope 'review PR diffs and flag conventions violations' \\
    --out-of-scope 'do refactors larger than the diff under review' \\
    --mindsets verify-before-act,right-amount \\
    --skills diff-review,test-author

What each flag does:

  --role STR           one-or-two-word agentRole (manifest field).
                       picker default: \"code reviewer\".

  --scope STR          one-line \"this agent DOES X\".
                       fills {{scopeDescription}} in AGENTS.md and
                       persona/README.md. Optional; empty leaves the
                       placeholder raw for manual edit later.

  --out-of-scope STR   one-line \"this agent DOES NOT do Y\".
                       fills {{outOfScope}}. Optional.

  --mindsets a,b,c     comma-separated kebab-case slugs. Creates
                       mindsets/<slug>.md per slug with the SPEC.md
                       scaffold (When to Apply / How to Apply /
                       When NOT to Apply / Related Principles).
                       Existing files are NOT clobbered.

  --skills a,b,c       comma-separated kebab-case slugs. Creates
                       skills/<slug>.md per slug (Domain / Inputs /
                       Outputs / Verification Gates / Out of Scope).
                       Maturity defaults to L1 per Ariya-dhana 7.

All four flags are optional. Press Enter through any prompt to skip.

The mindset and skill stubs are starting points — fill them in with
domain knowledge. The dashboard's detail pane and `bwoc status <name>`
both surface the resulting counts (mindsets/skills/memories) and the
persona scope inline.

Conceptual mapping:
  persona     —  WHO (Khandha 5: identity aggregate)
  mindsets    —  HOW (decision filters — Yoniso, Mattaññutā, Anattā)
  skills      —  DOES (capabilities — Ariya-dhana 7 maturity ladder)
  memories    —  KNOWS (Sappurisadhamma 7 — context held over time)

Edit any of these any time after incarnation — they're plain `.md`
under the agent's directory. The CLI doesn't gate edits; reads them
on demand.

See: bwoc help lifecycle  — when these get populated (new vs later)
     bwoc help manifest   — config.manifest.json schema (scope lives there too)
";

const MEMORY: &str = "\
Memory in BWOC is layered. The CLI surfaces the per-workspace tier
explicitly; the other tiers are accessed via file paths (today) or
will get their own CLI later.

Scope hierarchy:

  1. Per-agent       agents/<name>/memories/     (one agent's recall)
  2. Per-workspace   <workspace>/.bwoc/memory/   ← `bwoc memory` operates here
  3. Per-user        ~/.bwoc/memory/             (cross-workspace personal)
  4. Tier 2 backend  pluggable                   (Phase 2+)

`.bwoc/memory/` is scaffolded by `bwoc init` with a README explaining
the layout. The directory is plain Markdown — entries are files like
team-style.md, deploy-recipe.md.

Commands:

  bwoc memory list                    list entries (NAME / SIZE table)
  bwoc memory list --json             same as a JSON object
  bwoc memory show <name>             print one entry to stdout
  bwoc memory show --all              print every entry concatenated (with `# === <name> ===` headers)
  bwoc memory show --all --json       same as a JSON array of {name, content}
  bwoc memory put <name>              write from stdin (or `--file <p>`); `--force` overwrites
  bwoc memory search <query>          substring match across entries (case-insensitive)
  bwoc memory rm <name>               delete an entry; TTY-confirms unless `--yes` / `-y`

Entry name in `show` / `put` accepts `<name>` or `<name>.md`
interchangeably. Path traversal is refused: names with `/`, `\\`,
or a leading `.` exit 2 with an error before any file-system access.

`put` is atomic: stages to `<name>.md.tmp` then renames, so a
failed write never leaves a half-written entry. Refuses overwrite
without `--force`.

`search` prints `<name>:<line>:<content>` per match (grep-style)
or a JSON object with `--json`. Empty result is exit 0, not an error.

Read API for agents:
  Agents read these files like any other Markdown — no parsing,
  no SDK. The contract is `.bwoc/memory/*.md` exists in the workspace
  and is plain text. Agents using `bwoc spawn` to launch their
  backend CLI will have the workspace in their cwd, so reads are
  relative-path simple.

Write API for agents / users:
  `bwoc memory put <name>` accepts either `--file <path>` or stdin.
  Both shapes are common:
      echo 'team-style: 2-space indent' | bwoc memory put team-style
      bwoc memory put deploy --file ./deploy-recipe.md

  The directory is gitignored or tracked by team preference — there
  is no special ceremony.

What goes here:
  - Cross-agent conventions (\"all agents in this workspace use 2-space indent\")
  - Shared glossary terms
  - Deployment recipes, release procedures
  - Cross-cutting gotchas surfaced by one agent that others should know

What does NOT go here:
  - Per-agent specifics    → agents/<name>/memories/
  - Personal preferences   → ~/.bwoc/memory/
  - Secrets                → don't commit; use env vars or a vault

Excluded from `bwoc memory list`:
  - README.md (slot doc scaffolded by `bwoc init`)
  - non-`.md` files (the spec says plain Markdown)

See: bwoc help workspace  — full WORKSPACE.en.md spec including memory
     bwoc help persona    — per-agent persona (mindsets / skills / memories)
     bwoc help lifecycle  — when memory gets populated across an agent's life
";

const DOCTOR: &str = "\
`bwoc doctor` checks the environment + workspace for things that can
drift over time: stale daemon files, malformed cursors, oversized
logs. Each check produces one of four statuses:

  PASS    everything's fine
  WARN    issue exists but isn't blocking; advice in the detail
  FAIL    something is broken; rerun with --auto to attempt a fix
  FIXED   --auto saw a previous-FAIL/-WARN and repaired it

Exit code: 0 if no FAILs remain, 2 if any FAIL persists.

Checks (run automatically; no flag selection):

  Environment
    ~/.bwoc/                    Per-user config directory (created on
                                first run; --auto bootstraps it)
    backends on PATH            At least one of claude/agy/codex/kimi
                                discoverable (WARN if none — `bwoc spawn`
                                will fail without one)

  Workspace structure
    .bwoc/workspace.toml        Required marker; parse-checks fields
    .bwoc/agents.toml           Registry parse-check
    scaffold dirs               agents/, projects/, notes/, .bwoc/memory/

  Per-agent (when registry parses)
    agent symlinks              CLAUDE/AGY/CODEX/KIMI.md → AGENTS.md
                                (--auto recreates missing ones)
    agent.pid                   Stale sweep — PID file present but the
                                process is dead (signal-0)
    agent.sock                  Stale sweep — Unix socket present but no
                                live owner
    inbox.cursor                Sanity check — malformed (won't parse as
                                u64), out-of-bounds (> inbox size), or
                                orphan (cursor present + inbox missing)
    agent.log oversize          WARN if > 10 MiB (append-mode log; can
                                grow unbounded); --auto truncates in place
    inbox.jsonl oversize        WARN if > 5 MiB (user data — `--auto`
                                explicitly REFUSES to truncate; clear it
                                with `bwoc inbox <name> --clear` instead)

Flags:

  --workspace <path>            Override workspace resolution. Defaults
                                follow the standard chain (env, ancestor
                                walk, cwd).

  --auto                        Attempt to fix safe issues in place.
                                Never touches user data — agent.log
                                truncates (diagnostic chatter) but
                                inbox.jsonl is explicitly preserved.

  --json                        Stable structured output for CI gating:
                                  {
                                    \"results\": [{ name, status, detail }],
                                    \"summary\": { pass, warn, fail, fixed },
                                    \"exit\": 0|2
                                  }
                                `detail` is null for PASS, string otherwise.

Workflow:

  bwoc doctor                   First pass — see what's drifted
  bwoc doctor --auto            Fix the safe stuff
  bwoc doctor                   Confirm no FAILs remain
  bwoc inbox <agent> --clear    User-driven cleanup for inbox bloat

The doctor's policy: never silently discard user data. Diagnostic
chatter is fair game; messages are not.

See: bwoc help daemon     — what generates pid/sock/cursor in the first place
     bwoc help messaging  — inbox.jsonl lifecycle, --clear semantics
     bwoc help lifecycle  — overall state machine doctor diagnoses against
";

const SCRIPT: &str = "\
Recipes for driving `bwoc` from shell scripts. Every read-only
command has flags that strip output to integers, bare names, or
JSON — pick what your script needs.

Stripped-output flags (output mode precedence: count > names > default):

  bwoc list --count                  N (one integer)
  bwoc list --count --json           {\"count\": N}
  bwoc list --names-only             agent-foo\\n agent-bar\\n …
  bwoc list --names-only --json      {\"names\": [...]}
  bwoc list --json                   full agent objects + workspace

  bwoc memory list --count           N (memory entries, excluding README.md)
  bwoc memory list --names-only      filenames one per line

  bwoc inbox <agent> --count         envelope count
  bwoc inbox --all --json            { agents: [{ id, total, messages }] }

  bwoc workspace info --path-only    workspace root (for `cd \"$(...)\"`)
  bwoc workspace info --json         { workspace, defaults, agents, resources, attention }

Filters (combinable on `bwoc list`):

  bwoc list --running                ●-marked only
  bwoc list --status active          exact status match
  bwoc list --backend claude         exact backend match
  bwoc list --inbox-pending          agents with at least one envelope
  bwoc list --sort id|inbox|...      stable sort, registry-order default

Common idioms:

  # Stop everything for end of day
  bwoc stop --all --yes

  # Restart everyone (or just those that are stopped)
  bwoc start --all --yes

  # Find agents with unread work; print their counts
  for n in $(bwoc list --inbox-pending --names-only); do
    echo \"$n: $(bwoc inbox $n --count) pending\"
  done

  # CI gate: fail if any agent has a fail-status check
  bwoc check --all --json | jq -e '.summary.total_violations == 0'

  # CI gate: fail if any health probe fails
  bwoc doctor --json | jq -e '.exit == 0'

  # cd into workspace from anywhere
  cd \"$(bwoc workspace info --path-only)\"

  # Mass message inboxes (e.g. broadcast a system note)
  for n in $(bwoc list --names-only); do
    bwoc send $n \"system: maintenance window 14:00-15:00\"
  done

Exit codes (consistent across commands):

  0    success (or empty result, e.g. `bwoc list` with 0 agents)
  1    runtime error (failed IO, malformed JSON, etc.)
  2    user error (no workspace, missing agent, invalid arg, aborted)

Most read commands exit 0 even with zero results — \"nothing to show\"
is not an error. Doctor and check are the exceptions: they exit 2
if any FAIL / violation persists.

See: bwoc help getting-started  — install + first agent walkthrough
     bwoc help workspace        — resolution chain that all `--workspace`
                                  flags follow
     bwoc help messaging        — inbox flow that the shell loops above
                                  drive
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_nonempty_content() {
        for (name, summary, body) in TOPICS {
            assert!(!name.is_empty(), "topic name empty");
            assert!(!summary.is_empty(), "{name} summary empty");
            assert!(
                body.len() > 100,
                "{name} body too short ({} chars)",
                body.len()
            );
        }
    }

    #[test]
    fn topic_names_are_stable_slugs() {
        for (name, _, _) in TOPICS {
            for c in name.chars() {
                assert!(
                    c.is_ascii_lowercase() || c == '-',
                    "topic name '{name}' has invalid char '{c}'"
                );
            }
        }
    }

    #[test]
    fn unknown_topic_returns_exit_2() {
        let args = HelpArgs {
            topic: Some("nonexistent-zzz".to_string()),
            all: false,
        };
        // Can't easily assert exit code without capturing stdout/stderr,
        // but we can at least invoke without panicking.
        let code = run(args);
        assert_eq!(code, 2);
    }
}
