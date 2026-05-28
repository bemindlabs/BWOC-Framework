//! `bwoc council <verb>` — operator-facing CLI surface for the `council` plugin
//! kind (BWOC-58). Foundation of `BWOC-EPIC-5` (Agents Council).
//!
//! ## What this is
//!
//! The CLI half of the contract framed in
//! `notes/2026-05-28_council-plugin-architecture.md` (BWOC-56) and made
//! normative by the **Council Decision Schema** in `docs/en/PLUGINS.en.md`
//! (BWOC-57). `council` is the framework's first **coordination** plugin kind:
//! it acts neither outward (like `workflow`/`jira`) nor over the workspace as a
//! report (like `audit`/`okr`), but *among the fleet's own agents*. A decision
//! moves through `proposed → discussing → voting → resolved` (or `abandoned`
//! when quorum fails); participants are drawn from a `bwoc team`, discussion
//! turns route through `bwoc send` (the inbox is the transport, the record
//! references the envelope), and every entry conforms to the Council Decision
//! Schema.
//!
//! ## Where the work lives
//!
//! Unlike `bwoc okr` / `bwoc audit` (thin shells that delegate the whole
//! operation to the plugin's `entry` binary), `council` is **stateful across
//! invocations** — a decision opens, then accumulates turns and votes from
//! separate CLI calls before it resolves. So this module owns the decision
//! lifecycle and the on-disk record (`<workspace>/.bwoc/council/<id>.json`),
//! and the **tally is computed here** (a pure, unit-tested function over the
//! four voting models). The `council`-kind plugin's job is *declarative*: it
//! declares its `voting_model` + `quorum` in the manifest's `[council]` table,
//! which `propose` reads and snapshots into the decision at open time. This
//! keeps the protocol runnable for an L1 scripted exercise even before the
//! reference plugin's `entry` binary exists (BWOC-59, in flight) — naming a
//! not-yet-installed plugin exits `4` cleanly rather than panicking.
//!
//! ## Council plugin manifest contract (defined here, validated by BWOC-60)
//!
//! ```toml
//! [plugin]
//! name = "council-sangha-7"
//! kind = "council"
//! # … version / description / compat / entry as usual …
//!
//! [council]
//! voting_model = "sangha"   # simple-majority | consensus | weighted | sangha
//! quorum       = "2/3"      # positive integer, or an "n/m" fraction of the roster
//! ```
//!
//! ## Verb table
//!
//! | Verb                                                       | Needs plugin | Notes                                                            |
//! |---|---|---|
//! | `propose <plugin> --question --options a,b [--team t]`     | yes          | Opens a decision (`proposed`); reads the plugin's voting model + quorum. |
//! | `discuss <decision> --as <agent> --message <text>`         | no           | Appends a round turn; routes the turn through the inbox, records `message_ref`. |
//! | `vote <decision> --as <agent> (--option <x> \| --abstain)` | no           | Appends a vote (append-only; a re-cast appends, latest wins at tally). |
//! | `resolve <decision>`                                       | no           | Tallies per the snapshotted model, checks quorum, records outcome + dissent. |
//! | `list`                                                     | no           | Enumerate decisions in this workspace.                          |
//! | `show <decision>`                                          | no           | Print one decision's Council Decision Schema entry.             |
//!
//! ## Why no confirmation gate
//!
//! Every `council` write lands in the operator's own workspace
//! (`.bwoc/council/*.json`) or a teammate's inbox (`bwoc send` semantics —
//! itself ungated). Nothing mutates an external system of record, so — like
//! `bwoc okr track` — there is no confirmation prompt.
//!
//! ## Exit codes — normative
//!
//! - `0` — success (a `resolve` that closes the decision, whether `resolved`
//!   with an outcome or `abandoned` on quorum failure, is a success).
//! - `1` — local I/O error (record read/write, inbox delivery).
//! - `2` — operator/usage error (no workspace, unknown decision, non-member).
//! - `3` — `resolve` reached quorum but no decisive outcome under the model;
//!   the decision stays open for another discuss round (the framework never
//!   silently breaks a tie).
//! - `4` — the named `council` plugin is not installed in this workspace.
//! - `255` — plugin misconfiguration (manifest parse / missing `[council]`).
//!
//! Passing `--json` makes the exit code redundant: the envelope carries
//! `ok`/`status`/`error` with the same signal.

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use bwoc_core::team::Team;
use bwoc_core::workspace::AgentsRegistry;

// ---------------------------------------------------------------------------
// Exit codes + plugin kind (single source of truth).
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NOT_RESOLVED: i32 = 3;
const EXIT_NO_PLUGIN: i32 = 4;
const EXIT_PLUGIN_ERROR: i32 = 255;

const PLUGIN_KIND: &str = "council";

// Protocol states (Council Decision Schema `status`).
const STATUS_PROPOSED: &str = "proposed";
const STATUS_DISCUSSING: &str = "discussing";
const STATUS_VOTING: &str = "voting";
const STATUS_RESOLVED: &str = "resolved";
const STATUS_ABANDONED: &str = "abandoned";

// ---------------------------------------------------------------------------
// CLI surface — own arg structs so parsing is unit-testable against
// `CouncilCommand` directly (see `tests`).
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum CouncilCommand {
    /// Open a decision: a question + options, drawing participants from a team.
    Propose(ProposeArgs),
    /// Add a discussion turn to the current round; routes it through the inbox.
    Discuss(DiscussArgs),
    /// Cast (or re-cast) a vote for an option, or abstain. Append-only.
    Vote(VoteArgs),
    /// Tally per the decision's voting model, check quorum, record the outcome.
    Resolve(ResolveArgs),
    /// List the decisions recorded in this workspace.
    List(ListArgs),
    /// Print one decision's Council Decision Schema entry.
    Show(ShowArgs),
}

#[derive(Args, Debug)]
pub struct ProposeArgs {
    /// Council plugin name (directory under `modules/plugins/`). Supplies the
    /// `voting_model` + `quorum` from its `[council]` manifest table.
    plugin: String,
    /// The question the council decides.
    #[arg(long)]
    question: String,
    /// The options being decided among, comma-separated (≥2, e.g. `adopt,defer`).
    #[arg(long, value_delimiter = ',')]
    options: Vec<String>,
    /// Team whose members become the decision's participants. Without it the
    /// council is "open" — any agent may discuss/vote and quorum counts voters.
    #[arg(long)]
    team: Option<String>,
    /// `binding` (the outcome is authoritative; a follow-up `bwoc task` is the
    /// discipline) or `advisory` (a recommendation). Defaults to `advisory`.
    #[arg(long, default_value = "advisory")]
    effect: String,
    /// Optional evidence reference backing the decision (recorded as a
    /// `file`-kind evidence link, reusing the audit evidence model).
    #[arg(long)]
    evidence: Option<String>,
    /// Override the generated decision id (default `D<n>`). For scripting/tests.
    #[arg(long)]
    id: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DiscussArgs {
    /// The decision id (from `propose`).
    decision: String,
    /// The speaking participant (agent id; `agent-` prefix optional).
    #[arg(long = "as")]
    speaker: String,
    /// The turn's message. Routed through the inbox; the record stores a ref.
    #[arg(long)]
    message: String,
    /// Deliver only to this participant instead of all other participants.
    #[arg(long = "to")]
    to: Option<String>,
    /// Start a new discussion round for this turn instead of the current one.
    #[arg(long = "new-round")]
    new_round: bool,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct VoteArgs {
    /// The decision id.
    decision: String,
    /// The voting participant (agent id; `agent-` prefix optional).
    #[arg(long = "as")]
    voter: String,
    /// The option voted for. Mutually exclusive with `--abstain`.
    #[arg(long, conflicts_with = "abstain")]
    option: Option<String>,
    /// Record an abstention instead of an option.
    #[arg(long)]
    abstain: bool,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ResolveArgs {
    /// The decision id.
    decision: String,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Show only decisions in this status (e.g. `voting`, `resolved`).
    #[arg(long)]
    status: Option<String>,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// The decision id.
    decision: String,
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

/// Dispatch a parsed `CouncilCommand`. Returns the process exit code.
pub fn run(cmd: CouncilCommand) -> i32 {
    match cmd {
        CouncilCommand::Propose(a) => run_propose(a),
        CouncilCommand::Discuss(a) => run_discuss(a),
        CouncilCommand::Vote(a) => run_vote(a),
        CouncilCommand::Resolve(a) => run_resolve(a),
        CouncilCommand::List(a) => run_list(a),
        CouncilCommand::Show(a) => run_show(a),
    }
}

// ---------------------------------------------------------------------------
// Council Decision Schema record (superset). The 11 normative fields plus the
// operational metadata `propose` snapshots (plugin / question / voting_model /
// quorum / effect / team) so `resolve` can tally without re-reading the plugin.
// Optional fields are omitted when absent, per the framework convention.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Turn {
    participant: String,
    message_ref: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Round {
    round: u32,
    turns: Vec<Turn>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct VoteRecord {
    participant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    option: Option<String>,
    abstain: bool,
    /// Operational: append order for the audit trail (latest wins at tally).
    cast_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Dissent {
    participant: String,
    option: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rationale: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct EvidenceLink {
    kind: String,
    value: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Decision {
    decision_id: String,
    status: String,
    participants: Vec<String>,
    options: Vec<String>,
    rounds: Vec<Round>,
    votes: Vec<VoteRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dissent: Vec<Dissent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    evidence_links: Vec<EvidenceLink>,
    opened_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    closed_at: Option<String>,
    // --- operational metadata (outside the normative 11 fields) ---
    plugin: String,
    question: String,
    voting_model: String,
    quorum: String,
    effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    team: Option<String>,
}

impl Decision {
    /// The roster size quorum is computed against: the fixed participant set
    /// when a team was referenced, else the count of distinct voters (open
    /// council — quorum is relative to who showed up).
    fn roster_size(&self) -> usize {
        if self.participants.is_empty() {
            self.latest_votes().len()
        } else {
            self.participants.len()
        }
    }

    /// Collapse the append-only vote trail to the latest vote per participant,
    /// preserving first-seen order of participants. The trail keeps every cast;
    /// the tally honors only the most recent.
    fn latest_votes(&self) -> Vec<VoteRecord> {
        let mut order: Vec<String> = Vec::new();
        let mut latest: BTreeMap<String, VoteRecord> = BTreeMap::new();
        for v in &self.votes {
            if !latest.contains_key(&v.participant) {
                order.push(v.participant.clone());
            }
            latest.insert(v.participant.clone(), v.clone());
        }
        order
            .into_iter()
            .filter_map(|p| latest.remove(&p))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Voting models + quorum.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VotingModel {
    SimpleMajority,
    Consensus,
    Weighted,
    Sangha,
}

impl VotingModel {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "simple-majority" => Ok(Self::SimpleMajority),
            "consensus" => Ok(Self::Consensus),
            "weighted" => Ok(Self::Weighted),
            "sangha" => Ok(Self::Sangha),
            other => Err(format!(
                "unknown voting_model {other:?} (expected one of: \
                 simple-majority, consensus, weighted, sangha)"
            )),
        }
    }
}

/// A quorum declaration: a fixed count, or a fraction of the roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quorum {
    Count(u32),
    Fraction(u32, u32),
}

impl Quorum {
    fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if let Some((num, den)) = s.split_once('/') {
            let num: u32 = num
                .trim()
                .parse()
                .map_err(|_| format!("quorum numerator not an integer: {num:?}"))?;
            let den: u32 = den
                .trim()
                .parse()
                .map_err(|_| format!("quorum denominator not an integer: {den:?}"))?;
            if den == 0 {
                return Err("quorum fraction denominator must be > 0".to_string());
            }
            return Ok(Self::Fraction(num, den));
        }
        let n: u32 = s
            .parse()
            .map_err(|_| format!("quorum must be a positive integer or \"n/m\" fraction: {s:?}"))?;
        if n == 0 {
            return Err("quorum count must be > 0".to_string());
        }
        Ok(Self::Count(n))
    }

    /// Minimum voters required for the given roster size. A fraction rounds up
    /// (`2/3` of 4 → ceil(8/3) = 3); a count is capped at the roster size so an
    /// over-large declaration can still be met by a full turnout.
    fn required(&self, roster: usize) -> usize {
        match self {
            Self::Count(n) => (*n as usize).min(roster.max(1)),
            Self::Fraction(num, den) => {
                let need = (roster * (*num as usize)).div_ceil(*den as usize);
                need.max(1).min(roster.max(1))
            }
        }
    }
}

/// Validate a `[council].voting_model` value against the runtime tally's model
/// set. The single source of truth shared by the runtime (`discover_plugin`,
/// which routes through `VotingModel::parse` too) and the static manifest check
/// (`check.rs`, BWOC-60), so the static audit and the runtime cannot drift on
/// which models are recognized — the same contract `util::validate_plugin_entry`
/// establishes for the plugin entry path.
pub(crate) fn validate_voting_model(voting_model: &str) -> Result<(), String> {
    VotingModel::parse(voting_model).map(|_| ())
}

/// Validate a `[council].quorum` declaration (a TOML integer count or an `"n/m"`
/// fraction string) against the runtime parser. Shared single source of truth
/// with `discover_plugin` (`quorum_to_string` + `Quorum::parse`) so a quorum the
/// runtime would reject cannot pass `bwoc check`, and vice versa (BWOC-60).
pub(crate) fn validate_quorum(quorum: &toml::Value) -> Result<(), String> {
    Quorum::parse(&quorum_to_string(quorum)?).map(|_| ())
}

// ---------------------------------------------------------------------------
// Tally — pure, unit-tested. Decides resolved / abandoned / inconclusive.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Resolution {
    Resolved {
        outcome: String,
        dissent: Vec<Dissent>,
    },
    Abandoned {
        reason: String,
    },
    /// Quorum met but no decisive outcome under the model — the decision stays
    /// open for another discuss round (the framework never silently breaks a tie).
    Inconclusive {
        reason: String,
    },
}

/// Count non-abstaining votes per option, preserving first-seen order.
fn option_counts(latest: &[VoteRecord]) -> Vec<(String, usize)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for v in latest {
        if v.abstain {
            continue;
        }
        let Some(opt) = &v.option else { continue };
        if !counts.contains_key(opt) {
            order.push(opt.clone());
        }
        *counts.entry(opt.clone()).or_insert(0) += 1;
    }
    order
        .into_iter()
        .map(|o| {
            let c = counts.get(&o).copied().unwrap_or(0);
            (o, c)
        })
        .collect()
}

/// Everyone who cast a non-abstain vote for an option other than `outcome`.
fn dissenters(latest: &[VoteRecord], outcome: &str) -> Vec<Dissent> {
    latest
        .iter()
        .filter(|v| !v.abstain)
        .filter_map(|v| {
            v.option
                .as_ref()
                .map(|o| (v.participant.clone(), o.clone()))
        })
        .filter(|(_, opt)| opt != outcome)
        .map(|(participant, option)| Dissent {
            participant,
            option,
            rationale: None,
        })
        .collect()
}

/// Tally a decision's latest votes under `model`. `roster` is the quorum base.
fn tally(model: VotingModel, latest: &[VoteRecord], roster: usize, quorum: Quorum) -> Resolution {
    let voters = latest.len();
    let need = quorum.required(roster);
    if voters < need {
        return Resolution::Abandoned {
            reason: format!("quorum not met: {voters} voted, {need} required"),
        };
    }

    let counts = option_counts(latest);
    let cast: usize = counts.iter().map(|(_, c)| c).sum();
    if cast == 0 {
        return Resolution::Inconclusive {
            reason: "every voter abstained — no option to resolve to".to_string(),
        };
    }

    match model {
        VotingModel::SimpleMajority => {
            // Strictly more than half of the non-abstaining votes.
            if let Some((opt, _)) = counts.iter().find(|(_, c)| *c * 2 > cast) {
                Resolution::Resolved {
                    outcome: opt.clone(),
                    dissent: dissenters(latest, opt),
                }
            } else {
                Resolution::Inconclusive {
                    reason: "no option holds a majority (>50%) of cast votes — \
                             re-open a discuss round or let the operator decide"
                        .to_string(),
                }
            }
        }
        VotingModel::Consensus | VotingModel::Sangha => {
            // Unanimous assent among non-abstainers (abstentions allowed).
            let distinct: Vec<&(String, usize)> = counts.iter().filter(|(_, c)| *c > 0).collect();
            if distinct.len() == 1 {
                Resolution::Resolved {
                    outcome: distinct[0].0.clone(),
                    dissent: Vec::new(),
                }
            } else {
                let label = if model == VotingModel::Sangha {
                    "no concord — the quorum is not unanimous"
                } else {
                    "no consensus — voters are split across options"
                };
                Resolution::Inconclusive {
                    reason: format!("{label}; re-open a discuss round or abandon"),
                }
            }
        }
        VotingModel::Weighted => {
            // Equal weights (1 per voter) until a team weight field exists
            // (BWOC-57 deferred the weight source). Highest sum wins; a tie at
            // the top is inconclusive (no privileged vote to break it).
            let max = counts.iter().map(|(_, c)| *c).max().unwrap_or(0);
            let leaders: Vec<&(String, usize)> = counts.iter().filter(|(_, c)| *c == max).collect();
            if leaders.len() == 1 {
                let opt = leaders[0].0.clone();
                let dissent = dissenters(latest, &opt);
                Resolution::Resolved {
                    outcome: opt,
                    dissent,
                }
            } else {
                Resolution::Inconclusive {
                    reason: "weighted tie at the top (equal weights) — \
                             re-open a discuss round or let the operator decide"
                        .to_string(),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution — same shape as okr.rs / gcloud.rs / jira.rs.
// ---------------------------------------------------------------------------

fn find_workspace_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        let p = PathBuf::from(env_path);
        if !p.as_os_str().is_empty() {
            return Some(p);
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

fn resolve_workspace(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    find_workspace_root(explicit).ok_or_else(|| {
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
            .to_string()
    })
}

// ---------------------------------------------------------------------------
// Council plugin discovery — finds a `council`-kind plugin by name and reads
// its declared `[council]` voting model + quorum. Mirrors okr.rs's two-layout
// probe (flat `modules/plugins/<name>/` then `modules/plugins/council/<name>/`).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ManifestRaw {
    plugin: PluginSection,
    council: Option<CouncilSection>,
}

#[derive(Debug, Clone, Deserialize)]
struct PluginSection {
    name: String,
    kind: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CouncilSection {
    voting_model: String,
    quorum: toml::Value,
}

#[derive(Debug, Clone, PartialEq)]
struct CouncilPlugin {
    name: String,
    voting_model: String,
    quorum: String,
}

fn candidate_plugin_dirs(root: &Path, name: &str) -> [PathBuf; 2] {
    [
        root.join("modules/plugins").join(name),
        root.join("modules/plugins/council").join(name),
    ]
}

/// Normalize a `[council].quorum` TOML value to its string form.
fn quorum_to_string(v: &toml::Value) -> Result<String, String> {
    match v {
        toml::Value::Integer(n) if *n > 0 => Ok(n.to_string()),
        toml::Value::Integer(_) => Err("quorum integer must be > 0".to_string()),
        toml::Value::String(s) => Ok(s.clone()),
        other => Err(format!(
            "quorum must be a positive integer or \"n/m\" string, got {other:?}"
        )),
    }
}

/// Find a `council`-kind plugin by name. `Ok(None)` when no manifest matches;
/// `Err` when the plugin exists but is malformed/misconfigured (wrong kind, no
/// `[council]` table, bad quorum) — surface, never silently degrade.
fn discover_plugin(root: &Path, name: &str) -> Result<Option<CouncilPlugin>, String> {
    for plugin_dir in candidate_plugin_dirs(root, name) {
        let manifest = plugin_dir.join("manifest.toml");
        if !manifest.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("read {}: {e}", manifest.display()))?;
        let parsed: ManifestRaw =
            toml::from_str(&body).map_err(|e| format!("parse {}: {e}", manifest.display()))?;
        if parsed.plugin.name != name {
            continue;
        }
        if parsed.plugin.kind != PLUGIN_KIND {
            return Err(format!(
                "{}: [plugin].kind = {:?}, expected {:?}",
                manifest.display(),
                parsed.plugin.kind,
                PLUGIN_KIND
            ));
        }
        let Some(council) = parsed.council else {
            return Err(format!(
                "{}: missing [council] table (voting_model + quorum required for a council plugin)",
                manifest.display()
            ));
        };
        VotingModel::parse(&council.voting_model)
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
        let quorum = quorum_to_string(&council.quorum)
            .map_err(|e| format!("{}: [council].quorum: {e}", manifest.display()))?;
        Quorum::parse(&quorum)
            .map_err(|e| format!("{}: [council].quorum: {e}", manifest.display()))?;
        return Ok(Some(CouncilPlugin {
            name: parsed.plugin.name,
            voting_model: council.voting_model,
            quorum,
        }));
    }
    Ok(None)
}

fn no_plugin_message(plugin_name: &str) -> String {
    format!(
        "no installed '{plugin_name}' plugin (council kind) in this workspace. \
         A council's voting model + quorum are declared by a `council`-kind plugin \
         such as `council-sangha-7` (BWOC-59). Install it with \
         `bwoc plugin install <source>` then `bwoc plugin enable {plugin_name}`."
    )
}

// ---------------------------------------------------------------------------
// Decision record store — one JSON file per decision under .bwoc/council/.
// ---------------------------------------------------------------------------

fn council_dir(workspace: &Path) -> PathBuf {
    workspace.join(".bwoc/council")
}

fn decision_path(workspace: &Path, id: &str) -> PathBuf {
    council_dir(workspace).join(format!("{id}.json"))
}

fn load_decision(workspace: &Path, id: &str) -> Result<Option<Decision>, String> {
    let path = decision_path(workspace, id);
    match std::fs::read_to_string(&path) {
        Ok(body) => serde_json::from_str(&body)
            .map(Some)
            .map_err(|e| format!("decision '{id}' is malformed ({}): {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

/// Atomic write: render to a sibling tmp file then rename over the target.
fn save_decision(workspace: &Path, decision: &Decision) -> Result<(), String> {
    let dir = council_dir(workspace);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = decision_path(workspace, &decision.decision_id);
    let body =
        serde_json::to_string_pretty(decision).map_err(|e| format!("serialize decision: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename into {}: {e}", path.display()))?;
    Ok(())
}

/// Every decision in the workspace, sorted by id.
fn load_all_decisions(workspace: &Path) -> Result<Vec<Decision>, String> {
    let dir = council_dir(workspace);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out: Vec<Decision> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    entries.sort();
    for path in entries {
        let body =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let decision: Decision = serde_json::from_str(&body)
            .map_err(|e| format!("{} is malformed: {e}", path.display()))?;
        out.push(decision);
    }
    out.sort_by(|a, b| a.decision_id.cmp(&b.decision_id));
    Ok(out)
}

/// Next monotonic `D<n>` id not already on disk.
fn next_decision_id(workspace: &Path) -> String {
    let dir = council_dir(workspace);
    let mut max = 0u32;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for path in entries.filter_map(|r| r.ok()).map(|e| e.path()) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Some(num) = stem.strip_prefix('D')
                && let Ok(n) = num.parse::<u32>()
            {
                max = max.max(n);
            }
        }
    }
    format!("D{}", max + 1)
}

// ---------------------------------------------------------------------------
// Team + inbox integration.
// ---------------------------------------------------------------------------

/// Canonicalize a user-supplied agent name to its `agent-<name>` form
/// (mirrors `send::canonicalize`). Idempotent.
fn canonicalize(name: &str) -> String {
    if name.starts_with("agent-") {
        name.to_string()
    } else {
        format!("agent-{name}")
    }
}

/// Load a team's members (canonicalized) for use as decision participants.
fn team_participants(workspace: &Path, team_id: &str) -> Result<Vec<String>, String> {
    let path = workspace
        .join(".bwoc/teams")
        .join(format!("{team_id}.toml"));
    let body = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "no team '{team_id}' in workspace (expected {})",
            path.display()
        )
    })?;
    let team = Team::from_toml(&body).map_err(|e| format!("team '{team_id}' is malformed: {e}"))?;
    Ok(team.members.iter().map(|m| canonicalize(m)).collect())
}

/// Build a council turn message id (mirrors `send::generate_message_id`):
/// `msg-<utc-slug>-<5hex>`. A single id is shared across the fan-out delivery
/// so the turn has one `message_ref` regardless of how many inboxes receive it.
fn generate_message_id(ts: &str) -> String {
    let slug: String = ts.chars().filter(|c| *c != '-' && *c != ':').collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let suffix = nanos & 0xF_FFFF;
    format!("msg-{slug}-{suffix:05x}")
}

/// Deliver a discussion turn through the inbox transport — append one envelope
/// (sharing `message_id`) to each recipient's `.bwoc/inbox.jsonl`. Mirrors the
/// `bwoc send` envelope shape; the council record references the id, never a
/// copy of the body. Best-effort tmux wakeup is intentionally omitted (council
/// turns are operator/script-driven, not interactive). Returns the count
/// delivered. An empty recipient set (open council, lone speaker) is fine —
/// the turn is still recorded against the shared ref.
fn route_turn(
    workspace: &Path,
    from: &str,
    recipients: &[String],
    message: &str,
    message_id: &str,
    ts: &str,
) -> Result<usize, String> {
    if recipients.is_empty() {
        return Ok(0);
    }
    let registry =
        AgentsRegistry::load(workspace).map_err(|e| format!("load agent registry: {e}"))?;
    let mut delivered = 0usize;
    for to in recipients {
        let Some(entry) = registry.agents.iter().find(|a| a.id == *to) else {
            // A participant with no registry entry can't receive the turn.
            // Surface rather than silently drop — the roster references an
            // agent that isn't incarnated here.
            return Err(format!(
                "participant '{to}' is not a registered agent in this workspace"
            ));
        };
        let inbox = workspace.join(&entry.path).join(".bwoc/inbox.jsonl");
        if let Some(parent) = inbox.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let envelope = serde_json::json!({
            "ts": ts,
            "messageId": message_id,
            "from": from,
            "to": entry.id,
            "message": message,
            "kind": "council-turn",
        });
        let line = serde_json::to_string(&envelope).map_err(|e| format!("serialize turn: {e}"))?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inbox)
            .map_err(|e| format!("open {}: {e}", inbox.display()))?;
        writeln!(f, "{line}").map_err(|e| format!("write {}: {e}", inbox.display()))?;
        delivered += 1;
    }
    Ok(delivered)
}

// ---------------------------------------------------------------------------
// Shared output helpers.
// ---------------------------------------------------------------------------

fn print_json(value: &serde_json::Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(e) => {
            eprintln!("bwoc council: serialize JSON: {e}");
            false
        }
    }
}

fn emit_error_json(verb: &str, code: &str, message: &str) {
    let value = serde_json::json!({
        "ok": false,
        "verb": verb,
        "error": code,
        "message": message,
    });
    print_json(&value);
}

/// Serialize a decision to its Council Decision Schema JSON value.
fn decision_json(decision: &Decision) -> serde_json::Value {
    serde_json::to_value(decision).unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// Verb implementations.
// ---------------------------------------------------------------------------

fn run_propose(args: ProposeArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc council propose: {e}");
            return EXIT_USAGE;
        }
    };

    if args.options.len() < 2 {
        let msg = "a decision needs at least 2 options (--options a,b)";
        if args.json {
            emit_error_json("propose", "usage", msg);
        } else {
            eprintln!("bwoc council propose: {msg}");
        }
        return EXIT_USAGE;
    }
    if args.effect != "binding" && args.effect != "advisory" {
        let msg = format!(
            "--effect must be 'binding' or 'advisory', got {:?}",
            args.effect
        );
        if args.json {
            emit_error_json("propose", "usage", &msg);
        } else {
            eprintln!("bwoc council propose: {msg}");
        }
        return EXIT_USAGE;
    }

    // The plugin supplies the voting model + quorum (its [council] declaration).
    let plugin = match discover_plugin(&root, &args.plugin) {
        Ok(Some(p)) => p,
        Ok(None) => {
            let msg = no_plugin_message(&args.plugin);
            if args.json {
                emit_error_json("propose", "no_plugin", &msg);
            } else {
                eprintln!("bwoc council propose: {msg}");
            }
            return EXIT_NO_PLUGIN;
        }
        Err(e) => {
            if args.json {
                emit_error_json("propose", "plugin_error", &e);
            } else {
                eprintln!("bwoc council propose: {e}");
            }
            return EXIT_PLUGIN_ERROR;
        }
    };

    let participants = match &args.team {
        Some(team_id) => match team_participants(&root, team_id) {
            Ok(p) => p,
            Err(e) => {
                if args.json {
                    emit_error_json("propose", "team_error", &e);
                } else {
                    eprintln!("bwoc council propose: {e}");
                }
                return EXIT_USAGE;
            }
        },
        None => Vec::new(),
    };

    let id = args.id.unwrap_or_else(|| next_decision_id(&root));
    if load_decision(&root, &id).ok().flatten().is_some() {
        let msg = format!("decision '{id}' already exists");
        if args.json {
            emit_error_json("propose", "exists", &msg);
        } else {
            eprintln!("bwoc council propose: {msg}");
        }
        return EXIT_USAGE;
    }

    let evidence_links = args
        .evidence
        .map(|v| {
            vec![EvidenceLink {
                kind: "file".to_string(),
                value: v,
            }]
        })
        .unwrap_or_default();

    let decision = Decision {
        decision_id: id.clone(),
        status: STATUS_PROPOSED.to_string(),
        participants,
        options: args.options,
        rounds: Vec::new(),
        votes: Vec::new(),
        outcome: None,
        dissent: Vec::new(),
        evidence_links,
        opened_at: bwoc_core::time::utc_now_iso8601(),
        closed_at: None,
        plugin: plugin.name,
        question: args.question,
        voting_model: plugin.voting_model,
        quorum: plugin.quorum,
        effect: args.effect,
        team: args.team,
    };

    if let Err(e) = save_decision(&root, &decision) {
        if args.json {
            emit_error_json("propose", "io_error", &e);
        } else {
            eprintln!("bwoc council propose: {e}");
        }
        return EXIT_LOCAL_ERROR;
    }

    if args.json {
        return if print_json(&decision_json(&decision)) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }
    println!(
        "bwoc council propose: opened {} [{}] via plugin '{}'",
        decision.decision_id, decision.status, decision.plugin
    );
    println!("  question: {}", decision.question);
    println!("  options:  {}", decision.options.join(", "));
    println!(
        "  model:    {} (quorum {})  effect: {}",
        decision.voting_model, decision.quorum, decision.effect
    );
    if decision.participants.is_empty() {
        println!("  participants: (open — no team; voters self-identify with --as)");
    } else {
        println!("  participants: {}", decision.participants.join(", "));
    }
    EXIT_OK
}

fn run_discuss(args: DiscussArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc council discuss: {e}");
            return EXIT_USAGE;
        }
    };
    let mut decision = match require_open_decision(&root, &args.decision, "discuss", args.json) {
        Ok(d) => d,
        Err(code) => return code,
    };

    let speaker = canonicalize(&args.speaker);
    if let Err(code) = ensure_participant(&decision, &speaker, "discuss", args.json) {
        return code;
    }

    // Recipients: an explicit --to (validated), else every other participant.
    let recipients: Vec<String> = match &args.to {
        Some(to) => {
            let to = canonicalize(to);
            if !decision.participants.is_empty() && !decision.participants.contains(&to) {
                let msg = format!(
                    "--to '{to}' is not a participant of {}",
                    decision.decision_id
                );
                if args.json {
                    emit_error_json("discuss", "not_participant", &msg);
                } else {
                    eprintln!("bwoc council discuss: {msg}");
                }
                return EXIT_USAGE;
            }
            vec![to]
        }
        None => decision
            .participants
            .iter()
            .filter(|p| **p != speaker)
            .cloned()
            .collect(),
    };

    let ts = bwoc_core::time::utc_now_iso8601();
    let message_id = generate_message_id(&ts);
    let delivered = match route_turn(
        &root,
        &speaker,
        &recipients,
        &args.message,
        &message_id,
        &ts,
    ) {
        Ok(n) => n,
        Err(e) => {
            if args.json {
                emit_error_json("discuss", "delivery_error", &e);
            } else {
                eprintln!("bwoc council discuss: {e}");
            }
            return EXIT_LOCAL_ERROR;
        }
    };

    // Append the turn to the current round (or a fresh one).
    let turn = Turn {
        participant: speaker.clone(),
        message_ref: message_id.clone(),
    };
    if args.new_round || decision.rounds.is_empty() {
        let next = decision.rounds.len() as u32 + 1;
        decision.rounds.push(Round {
            round: next,
            turns: vec![turn],
        });
    } else {
        let last = decision.rounds.len() - 1;
        decision.rounds[last].turns.push(turn);
    }
    if decision.status == STATUS_PROPOSED {
        decision.status = STATUS_DISCUSSING.to_string();
    }

    if let Err(e) = save_decision(&root, &decision) {
        if args.json {
            emit_error_json("discuss", "io_error", &e);
        } else {
            eprintln!("bwoc council discuss: {e}");
        }
        return EXIT_LOCAL_ERROR;
    }

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "decision_id": decision.decision_id,
            "status": decision.status,
            "round": decision.rounds.last().map(|r| r.round).unwrap_or(1),
            "message_ref": message_id,
            "delivered": delivered,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }
    let round = decision.rounds.last().map(|r| r.round).unwrap_or(1);
    println!(
        "bwoc council discuss: {} round {round} — {speaker} spoke (ref {message_id}, delivered to {delivered})",
        decision.decision_id
    );
    EXIT_OK
}

fn run_vote(args: VoteArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc council vote: {e}");
            return EXIT_USAGE;
        }
    };
    if !args.abstain && args.option.is_none() {
        let msg = "pass --option <x> or --abstain";
        if args.json {
            emit_error_json("vote", "usage", msg);
        } else {
            eprintln!("bwoc council vote: {msg}");
        }
        return EXIT_USAGE;
    }
    let mut decision = match require_open_decision(&root, &args.decision, "vote", args.json) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let voter = canonicalize(&args.voter);
    if let Err(code) = ensure_participant(&decision, &voter, "vote", args.json) {
        return code;
    }
    if let Some(opt) = &args.option
        && !decision.options.contains(opt)
    {
        let msg = format!(
            "option {opt:?} is not one of the decision's options ({})",
            decision.options.join(", ")
        );
        if args.json {
            emit_error_json("vote", "bad_option", &msg);
        } else {
            eprintln!("bwoc council vote: {msg}");
        }
        return EXIT_USAGE;
    }

    decision.votes.push(VoteRecord {
        participant: voter.clone(),
        option: if args.abstain {
            None
        } else {
            args.option.clone()
        },
        abstain: args.abstain,
        cast_at: bwoc_core::time::utc_now_iso8601(),
    });
    if decision.status == STATUS_PROPOSED || decision.status == STATUS_DISCUSSING {
        decision.status = STATUS_VOTING.to_string();
    }

    if let Err(e) = save_decision(&root, &decision) {
        if args.json {
            emit_error_json("vote", "io_error", &e);
        } else {
            eprintln!("bwoc council vote: {e}");
        }
        return EXIT_LOCAL_ERROR;
    }

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "decision_id": decision.decision_id,
            "status": decision.status,
            "participant": voter,
            "option": args.option,
            "abstain": args.abstain,
            "votes_cast": decision.votes.len(),
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }
    let what = if args.abstain {
        "abstained".to_string()
    } else {
        format!("voted {:?}", args.option.as_deref().unwrap_or(""))
    };
    println!(
        "bwoc council vote: {} — {voter} {what} ({} vote(s) cast)",
        decision.decision_id,
        decision.votes.len()
    );
    EXIT_OK
}

fn run_resolve(args: ResolveArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc council resolve: {e}");
            return EXIT_USAGE;
        }
    };
    let mut decision = match load_decision(&root, &args.decision) {
        Ok(Some(d)) => d,
        Ok(None) => {
            let msg = format!("no decision '{}' in this workspace", args.decision);
            if args.json {
                emit_error_json("resolve", "not_found", &msg);
            } else {
                eprintln!("bwoc council resolve: {msg}");
            }
            return EXIT_USAGE;
        }
        Err(e) => {
            if args.json {
                emit_error_json("resolve", "io_error", &e);
            } else {
                eprintln!("bwoc council resolve: {e}");
            }
            return EXIT_LOCAL_ERROR;
        }
    };

    if decision.status == STATUS_RESOLVED || decision.status == STATUS_ABANDONED {
        let msg = format!(
            "decision '{}' is already {} (closed at {})",
            decision.decision_id,
            decision.status,
            decision.closed_at.as_deref().unwrap_or("?")
        );
        if args.json {
            emit_error_json("resolve", "already_closed", &msg);
        } else {
            eprintln!("bwoc council resolve: {msg}");
        }
        return EXIT_USAGE;
    }

    let model = match VotingModel::parse(&decision.voting_model) {
        Ok(m) => m,
        Err(e) => {
            if args.json {
                emit_error_json("resolve", "bad_model", &e);
            } else {
                eprintln!("bwoc council resolve: {e}");
            }
            return EXIT_PLUGIN_ERROR;
        }
    };
    let quorum = match Quorum::parse(&decision.quorum) {
        Ok(q) => q,
        Err(e) => {
            if args.json {
                emit_error_json("resolve", "bad_quorum", &e);
            } else {
                eprintln!("bwoc council resolve: {e}");
            }
            return EXIT_PLUGIN_ERROR;
        }
    };

    let latest = decision.latest_votes();
    let roster = decision.roster_size();
    let resolution = tally(model, &latest, roster, quorum);

    match resolution {
        Resolution::Resolved { outcome, dissent } => {
            decision.status = STATUS_RESOLVED.to_string();
            decision.outcome = Some(outcome.clone());
            decision.dissent = dissent;
            decision.closed_at = Some(bwoc_core::time::utc_now_iso8601());
            if let Err(code) = persist_resolution(&root, &decision, "resolve", args.json) {
                return code;
            }
            if args.json {
                return if print_json(&decision_json(&decision)) {
                    EXIT_OK
                } else {
                    EXIT_LOCAL_ERROR
                };
            }
            println!(
                "bwoc council resolve: {} resolved → {outcome} (model {}, {} voter(s))",
                decision.decision_id,
                decision.voting_model,
                latest.len()
            );
            if !decision.dissent.is_empty() {
                let names: Vec<String> = decision
                    .dissent
                    .iter()
                    .map(|d| format!("{} ({})", d.participant, d.option))
                    .collect();
                println!("  dissent: {}", names.join(", "));
            }
            if decision.effect == "binding" {
                println!(
                    "  effect: binding — emit a follow-up `bwoc task` to carry out the outcome \
                     (council records, it does not execute)."
                );
            }
            EXIT_OK
        }
        Resolution::Abandoned { reason } => {
            decision.status = STATUS_ABANDONED.to_string();
            decision.closed_at = Some(bwoc_core::time::utc_now_iso8601());
            if let Err(code) = persist_resolution(&root, &decision, "resolve", args.json) {
                return code;
            }
            if args.json {
                return if print_json(&decision_json(&decision)) {
                    EXIT_OK
                } else {
                    EXIT_LOCAL_ERROR
                };
            }
            println!(
                "bwoc council resolve: {} abandoned — {reason}",
                decision.decision_id
            );
            EXIT_OK
        }
        Resolution::Inconclusive { reason } => {
            // Leave the decision open; the operator runs another discuss round.
            if args.json {
                let value = serde_json::json!({
                    "ok": false,
                    "verb": "resolve",
                    "decision_id": decision.decision_id,
                    "status": decision.status,
                    "resolved": false,
                    "reason": reason,
                });
                print_json(&value);
            } else {
                println!(
                    "bwoc council resolve: {} not resolved — {reason}",
                    decision.decision_id
                );
            }
            EXIT_NOT_RESOLVED
        }
    }
}

fn run_list(args: ListArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc council list: {e}");
            return EXIT_USAGE;
        }
    };
    let decisions = match load_all_decisions(&root) {
        Ok(d) => d,
        Err(e) => {
            if args.json {
                emit_error_json("list", "io_error", &e);
            } else {
                eprintln!("bwoc council list: {e}");
            }
            return EXIT_LOCAL_ERROR;
        }
    };
    let rows: Vec<&Decision> = decisions
        .iter()
        .filter(|d| args.status.as_ref().is_none_or(|s| d.status == *s))
        .collect();

    if args.json {
        let arr: Vec<serde_json::Value> = rows
            .iter()
            .map(|d| {
                serde_json::json!({
                    "decision_id": d.decision_id,
                    "status": d.status,
                    "question": d.question,
                    "plugin": d.plugin,
                    "voting_model": d.voting_model,
                    "options": d.options,
                    "votes_cast": d.votes.len(),
                    "outcome": d.outcome,
                })
            })
            .collect();
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "decisions": arr,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }

    println!("bwoc council list: {} decision(s)", rows.len());
    for d in &rows {
        let outcome = d.outcome.as_deref().unwrap_or("—");
        println!(
            "  {:<6} {:<11} {} → {}  [{}]",
            d.decision_id, d.status, d.question, outcome, d.voting_model
        );
    }
    EXIT_OK
}

fn run_show(args: ShowArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc council show: {e}");
            return EXIT_USAGE;
        }
    };
    let decision = match load_decision(&root, &args.decision) {
        Ok(Some(d)) => d,
        Ok(None) => {
            let msg = format!("no decision '{}' in this workspace", args.decision);
            if args.json {
                emit_error_json("show", "not_found", &msg);
            } else {
                eprintln!("bwoc council show: {msg}");
            }
            return EXIT_USAGE;
        }
        Err(e) => {
            if args.json {
                emit_error_json("show", "io_error", &e);
            } else {
                eprintln!("bwoc council show: {e}");
            }
            return EXIT_LOCAL_ERROR;
        }
    };

    if args.json {
        return if print_json(&decision_json(&decision)) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }

    println!(
        "bwoc council show: {} [{}]",
        decision.decision_id, decision.status
    );
    println!("  question: {}", decision.question);
    println!("  options:  {}", decision.options.join(", "));
    println!(
        "  model:    {} (quorum {})  effect: {}  plugin: {}",
        decision.voting_model, decision.quorum, decision.effect, decision.plugin
    );
    if let Some(team) = &decision.team {
        println!("  team:     {team}");
    }
    if decision.participants.is_empty() {
        println!("  participants: (open)");
    } else {
        println!("  participants: {}", decision.participants.join(", "));
    }
    println!("  opened:   {}", decision.opened_at);
    if let Some(c) = &decision.closed_at {
        println!("  closed:   {c}");
    }
    if decision.rounds.is_empty() {
        println!("  rounds:   (none)");
    } else {
        for r in &decision.rounds {
            let speakers: Vec<&str> = r.turns.iter().map(|t| t.participant.as_str()).collect();
            println!(
                "  round {}: {} turn(s) — {}",
                r.round,
                r.turns.len(),
                speakers.join(", ")
            );
        }
    }
    if decision.votes.is_empty() {
        println!("  votes:    (none)");
    } else {
        for v in &decision.latest_votes() {
            let choice = if v.abstain {
                "abstain".to_string()
            } else {
                v.option.clone().unwrap_or_default()
            };
            println!("  vote:     {} → {choice}", v.participant);
        }
    }
    if let Some(outcome) = &decision.outcome {
        println!("  outcome:  {outcome}");
    }
    for d in &decision.dissent {
        println!("  dissent:  {} ({})", d.participant, d.option);
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// Shared verb helpers.
// ---------------------------------------------------------------------------

/// Load a decision that is still open (not resolved/abandoned). Maps absence to
/// usage exit 2 and a closed decision to usage exit 2 with a clear message.
fn require_open_decision(root: &Path, id: &str, verb: &str, json: bool) -> Result<Decision, i32> {
    match load_decision(root, id) {
        Ok(Some(d)) => {
            if d.status == STATUS_RESOLVED || d.status == STATUS_ABANDONED {
                let msg = format!(
                    "decision '{id}' is closed ({}) — no further {verb}",
                    d.status
                );
                if json {
                    emit_error_json(verb, "closed", &msg);
                } else {
                    eprintln!("bwoc council {verb}: {msg}");
                }
                return Err(EXIT_USAGE);
            }
            Ok(d)
        }
        Ok(None) => {
            let msg = format!(
                "no decision '{id}' in this workspace (open one with `bwoc council propose`)"
            );
            if json {
                emit_error_json(verb, "not_found", &msg);
            } else {
                eprintln!("bwoc council {verb}: {msg}");
            }
            Err(EXIT_USAGE)
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "io_error", &e);
            } else {
                eprintln!("bwoc council {verb}: {e}");
            }
            Err(EXIT_LOCAL_ERROR)
        }
    }
}

/// On a team-bound decision, the actor must be a participant. An open council
/// (no participants) accepts any agent.
fn ensure_participant(decision: &Decision, agent: &str, verb: &str, json: bool) -> Result<(), i32> {
    if decision.participants.is_empty() || decision.participants.iter().any(|p| p == agent) {
        return Ok(());
    }
    let msg = format!(
        "'{agent}' is not a participant of {} (participants: {})",
        decision.decision_id,
        decision.participants.join(", ")
    );
    if json {
        emit_error_json(verb, "not_participant", &msg);
    } else {
        eprintln!("bwoc council {verb}: {msg}");
    }
    Err(EXIT_USAGE)
}

fn persist_resolution(root: &Path, decision: &Decision, verb: &str, json: bool) -> Result<(), i32> {
    if let Err(e) = save_decision(root, decision) {
        if json {
            emit_error_json(verb, "io_error", &e);
        } else {
            eprintln!("bwoc council {verb}: {e}");
        }
        return Err(EXIT_LOCAL_ERROR);
    }
    Ok(())
}

// ===========================================================================
// Tests — arg parsing, plugin discovery, quorum/voting-model parsing, the pure
// tally across all four models + quorum, the decision store round-trip, and the
// latest-vote collapse.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        cmd: CouncilCommand,
    }

    fn parse(args: &[&str]) -> Result<CouncilCommand, clap::Error> {
        let mut full = vec!["bwoc-council-test"];
        full.extend_from_slice(args);
        TestCli::try_parse_from(full).map(|c| c.cmd)
    }

    // --- arg parsing -------------------------------------------------------

    #[test]
    fn parses_propose_with_options_csv() {
        match parse(&[
            "propose",
            "council-sangha-7",
            "--question",
            "Adopt X?",
            "--options",
            "adopt,defer",
            "--team",
            "core",
            "--effect",
            "binding",
        ])
        .unwrap()
        {
            CouncilCommand::Propose(a) => {
                assert_eq!(a.plugin, "council-sangha-7");
                assert_eq!(a.question, "Adopt X?");
                assert_eq!(a.options, vec!["adopt", "defer"]);
                assert_eq!(a.team.as_deref(), Some("core"));
                assert_eq!(a.effect, "binding");
            }
            other => panic!("expected Propose, got {other:?}"),
        }
    }

    #[test]
    fn propose_effect_defaults_to_advisory() {
        match parse(&["propose", "p", "--question", "q?", "--options", "a,b"]).unwrap() {
            CouncilCommand::Propose(a) => assert_eq!(a.effect, "advisory"),
            other => panic!("expected Propose, got {other:?}"),
        }
    }

    #[test]
    fn parses_discuss() {
        match parse(&[
            "discuss",
            "D1",
            "--as",
            "jennie",
            "--message",
            "I prefer adopt",
            "--new-round",
        ])
        .unwrap()
        {
            CouncilCommand::Discuss(a) => {
                assert_eq!(a.decision, "D1");
                assert_eq!(a.speaker, "jennie");
                assert_eq!(a.message, "I prefer adopt");
                assert!(a.new_round);
            }
            other => panic!("expected Discuss, got {other:?}"),
        }
    }

    #[test]
    fn parses_vote_option_and_abstain() {
        match parse(&["vote", "D1", "--as", "lisa", "--option", "adopt"]).unwrap() {
            CouncilCommand::Vote(a) => {
                assert_eq!(a.option.as_deref(), Some("adopt"));
                assert!(!a.abstain);
            }
            other => panic!("expected Vote, got {other:?}"),
        }
        match parse(&["vote", "D1", "--as", "lisa", "--abstain"]).unwrap() {
            CouncilCommand::Vote(a) => {
                assert!(a.abstain);
                assert!(a.option.is_none());
            }
            other => panic!("expected Vote, got {other:?}"),
        }
    }

    #[test]
    fn vote_option_and_abstain_conflict() {
        assert!(parse(&["vote", "D1", "--as", "x", "--option", "a", "--abstain"]).is_err());
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(parse(&["frobnicate"]).is_err());
    }

    // --- canonicalize ------------------------------------------------------

    #[test]
    fn canonicalize_is_idempotent() {
        assert_eq!(canonicalize("jennie"), "agent-jennie");
        assert_eq!(canonicalize("agent-jennie"), "agent-jennie");
    }

    // --- voting model + quorum parsing -------------------------------------

    #[test]
    fn voting_model_parses_the_four() {
        assert_eq!(
            VotingModel::parse("simple-majority").unwrap(),
            VotingModel::SimpleMajority
        );
        assert_eq!(
            VotingModel::parse("consensus").unwrap(),
            VotingModel::Consensus
        );
        assert_eq!(
            VotingModel::parse("weighted").unwrap(),
            VotingModel::Weighted
        );
        assert_eq!(VotingModel::parse("sangha").unwrap(), VotingModel::Sangha);
        assert!(VotingModel::parse("dictatorship").is_err());
    }

    #[test]
    fn quorum_parses_count_and_fraction() {
        assert_eq!(Quorum::parse("3").unwrap(), Quorum::Count(3));
        assert_eq!(Quorum::parse("2/3").unwrap(), Quorum::Fraction(2, 3));
        assert!(Quorum::parse("0").is_err());
        assert!(Quorum::parse("1/0").is_err());
        assert!(Quorum::parse("two").is_err());
    }

    #[test]
    fn quorum_required_rounds_fraction_up() {
        // 2/3 of 4 = 2.67 → 3
        assert_eq!(Quorum::Fraction(2, 3).required(4), 3);
        // 1/2 of 4 = 2
        assert_eq!(Quorum::Fraction(1, 2).required(4), 2);
        // full unanimity
        assert_eq!(Quorum::Fraction(1, 1).required(4), 4);
        // count capped at roster
        assert_eq!(Quorum::Count(10).required(4), 4);
        assert_eq!(Quorum::Count(2).required(4), 2);
    }

    #[test]
    fn quorum_to_string_normalizes() {
        assert_eq!(quorum_to_string(&toml::Value::Integer(3)).unwrap(), "3");
        assert_eq!(
            quorum_to_string(&toml::Value::String("2/3".into())).unwrap(),
            "2/3"
        );
        assert!(quorum_to_string(&toml::Value::Integer(0)).is_err());
        assert!(quorum_to_string(&toml::Value::Boolean(true)).is_err());
    }

    // --- tally: helpers to build votes -------------------------------------

    fn vote(p: &str, opt: Option<&str>) -> VoteRecord {
        VoteRecord {
            participant: p.to_string(),
            option: opt.map(|s| s.to_string()),
            abstain: opt.is_none(),
            cast_at: "2026-05-28T12:00:00Z".to_string(),
        }
    }

    // --- tally: quorum -----------------------------------------------------

    #[test]
    fn tally_abandons_when_quorum_unmet() {
        let votes = vec![vote("a", Some("adopt"))];
        let r = tally(
            VotingModel::SimpleMajority,
            &votes,
            4,
            Quorum::Fraction(2, 3),
        );
        match r {
            Resolution::Abandoned { reason } => assert!(reason.contains("quorum"), "{reason}"),
            other => panic!("expected Abandoned, got {other:?}"),
        }
    }

    // --- tally: simple-majority --------------------------------------------

    #[test]
    fn simple_majority_resolves_over_half() {
        let votes = vec![
            vote("a", Some("adopt")),
            vote("b", Some("adopt")),
            vote("c", Some("defer")),
        ];
        let r = tally(VotingModel::SimpleMajority, &votes, 3, Quorum::Count(2));
        match r {
            Resolution::Resolved { outcome, dissent } => {
                assert_eq!(outcome, "adopt");
                assert_eq!(dissent.len(), 1);
                assert_eq!(dissent[0].participant, "c");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn simple_majority_inconclusive_on_tie() {
        let votes = vec![vote("a", Some("adopt")), vote("b", Some("defer"))];
        let r = tally(VotingModel::SimpleMajority, &votes, 2, Quorum::Count(2));
        assert!(matches!(r, Resolution::Inconclusive { .. }), "{r:?}");
    }

    // --- tally: consensus + sangha -----------------------------------------

    #[test]
    fn consensus_resolves_only_when_unanimous() {
        let unanimous = vec![
            vote("a", Some("adopt")),
            vote("b", Some("adopt")),
            vote("c", None), // abstain allowed
        ];
        match tally(VotingModel::Consensus, &unanimous, 3, Quorum::Count(2)) {
            Resolution::Resolved { outcome, dissent } => {
                assert_eq!(outcome, "adopt");
                assert!(dissent.is_empty());
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
        let split = vec![vote("a", Some("adopt")), vote("b", Some("defer"))];
        assert!(matches!(
            tally(VotingModel::Consensus, &split, 2, Quorum::Count(2)),
            Resolution::Inconclusive { .. }
        ));
    }

    #[test]
    fn sangha_requires_concord() {
        let concord = vec![vote("a", Some("honor")), vote("b", Some("honor"))];
        assert!(matches!(
            tally(VotingModel::Sangha, &concord, 2, Quorum::Fraction(1, 1)),
            Resolution::Resolved { .. }
        ));
        let discord = vec![vote("a", Some("honor")), vote("b", Some("revise"))];
        match tally(VotingModel::Sangha, &discord, 2, Quorum::Count(2)) {
            Resolution::Inconclusive { reason } => assert!(reason.contains("concord"), "{reason}"),
            other => panic!("expected Inconclusive, got {other:?}"),
        }
    }

    #[test]
    fn all_abstain_is_inconclusive() {
        let votes = vec![vote("a", None), vote("b", None)];
        assert!(matches!(
            tally(VotingModel::Consensus, &votes, 2, Quorum::Count(2)),
            Resolution::Inconclusive { .. }
        ));
    }

    // --- tally: weighted ---------------------------------------------------

    #[test]
    fn weighted_resolves_plurality_and_ties_inconclusive() {
        // Plurality (not majority) wins under equal weights.
        let plurality = vec![
            vote("a", Some("x")),
            vote("b", Some("x")),
            vote("c", Some("y")),
            vote("d", Some("z")),
        ];
        match tally(VotingModel::Weighted, &plurality, 4, Quorum::Count(3)) {
            Resolution::Resolved { outcome, dissent } => {
                assert_eq!(outcome, "x");
                assert_eq!(dissent.len(), 2);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
        let tie = vec![vote("a", Some("x")), vote("b", Some("y"))];
        assert!(matches!(
            tally(VotingModel::Weighted, &tie, 2, Quorum::Count(2)),
            Resolution::Inconclusive { .. }
        ));
    }

    // --- latest-vote collapse ----------------------------------------------

    #[test]
    fn latest_vote_per_participant_wins() {
        let decision = Decision {
            decision_id: "D1".into(),
            status: STATUS_VOTING.into(),
            participants: vec!["agent-a".into(), "agent-b".into()],
            options: vec!["adopt".into(), "defer".into()],
            rounds: vec![],
            votes: vec![
                vote("agent-a", Some("defer")),
                vote("agent-a", Some("adopt")), // re-cast — latest wins
                vote("agent-b", Some("adopt")),
            ],
            outcome: None,
            dissent: vec![],
            evidence_links: vec![],
            opened_at: "2026-05-28T12:00:00Z".into(),
            closed_at: None,
            plugin: "council-sangha-7".into(),
            question: "q?".into(),
            voting_model: "consensus".into(),
            quorum: "2".into(),
            effect: "advisory".into(),
            team: Some("core".into()),
        };
        let latest = decision.latest_votes();
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].option.as_deref(), Some("adopt"));
        // Now unanimous → consensus resolves.
        assert!(matches!(
            tally(VotingModel::Consensus, &latest, 2, Quorum::Count(2)),
            Resolution::Resolved { .. }
        ));
    }

    // --- plugin discovery --------------------------------------------------

    fn write_council_plugin(root: &Path, layout: &str, name: &str, kind: &str, council: &str) {
        let dir = root.join("modules/plugins").join(layout).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "[plugin]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
                 description = \"a council\"\ncompat = \">=2.5.0\"\nentry = \"council.sh\"\n{council}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn discovers_council_plugin_with_declaration() {
        let dir = tempfile::tempdir().unwrap();
        write_council_plugin(
            dir.path(),
            "",
            "council-sangha-7",
            "council",
            "\n[council]\nvoting_model = \"sangha\"\nquorum = \"2/3\"\n",
        );
        let p = discover_plugin(dir.path(), "council-sangha-7")
            .unwrap()
            .unwrap();
        assert_eq!(p.name, "council-sangha-7");
        assert_eq!(p.voting_model, "sangha");
        assert_eq!(p.quorum, "2/3");
    }

    #[test]
    fn discovers_council_namespaced_layout_with_integer_quorum() {
        let dir = tempfile::tempdir().unwrap();
        write_council_plugin(
            dir.path(),
            "council",
            "ops-council",
            "council",
            "\n[council]\nvoting_model = \"simple-majority\"\nquorum = 3\n",
        );
        let p = discover_plugin(dir.path(), "ops-council").unwrap().unwrap();
        assert_eq!(p.quorum, "3");
        assert_eq!(p.voting_model, "simple-majority");
    }

    #[test]
    fn discovery_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_plugin(dir.path(), "nope").unwrap().is_none());
    }

    #[test]
    fn discovery_rejects_wrong_kind() {
        let dir = tempfile::tempdir().unwrap();
        write_council_plugin(dir.path(), "", "x", "audit", "");
        let err = discover_plugin(dir.path(), "x").unwrap_err();
        assert!(err.contains("expected"), "{err}");
        assert!(err.contains("council"), "{err}");
    }

    #[test]
    fn discovery_rejects_missing_council_table() {
        let dir = tempfile::tempdir().unwrap();
        write_council_plugin(dir.path(), "", "x", "council", "");
        let err = discover_plugin(dir.path(), "x").unwrap_err();
        assert!(err.contains("[council]"), "{err}");
    }

    #[test]
    fn discovery_rejects_bad_voting_model() {
        let dir = tempfile::tempdir().unwrap();
        write_council_plugin(
            dir.path(),
            "",
            "x",
            "council",
            "\n[council]\nvoting_model = \"king\"\nquorum = 2\n",
        );
        let err = discover_plugin(dir.path(), "x").unwrap_err();
        assert!(
            err.contains("voting_model") || err.contains("king"),
            "{err}"
        );
    }

    // --- decision store round-trip -----------------------------------------

    fn write_workspace(root: &Path) {
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(
            root.join(".bwoc/workspace.toml"),
            "[workspace]\nname = \"t\"\nversion = \"0.1.0\"\ncreated = \"2026-05-28T00:00:00Z\"\n",
        )
        .unwrap();
    }

    fn sample_decision(id: &str) -> Decision {
        Decision {
            decision_id: id.into(),
            status: STATUS_PROPOSED.into(),
            participants: vec!["agent-a".into(), "agent-b".into()],
            options: vec!["adopt".into(), "defer".into()],
            rounds: vec![],
            votes: vec![],
            outcome: None,
            dissent: vec![],
            evidence_links: vec![],
            opened_at: "2026-05-28T12:00:00Z".into(),
            closed_at: None,
            plugin: "council-sangha-7".into(),
            question: "Adopt X?".into(),
            voting_model: "consensus".into(),
            quorum: "2".into(),
            effect: "advisory".into(),
            team: Some("core".into()),
        }
    }

    #[test]
    fn decision_store_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path());
        let d = sample_decision("D1");
        save_decision(dir.path(), &d).unwrap();
        let back = load_decision(dir.path(), "D1").unwrap().unwrap();
        assert_eq!(back, d);
        assert!(load_decision(dir.path(), "D99").unwrap().is_none());
    }

    #[test]
    fn next_decision_id_is_monotonic() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path());
        assert_eq!(next_decision_id(dir.path()), "D1");
        save_decision(dir.path(), &sample_decision("D1")).unwrap();
        save_decision(dir.path(), &sample_decision("D2")).unwrap();
        assert_eq!(next_decision_id(dir.path()), "D3");
    }

    #[test]
    fn load_all_decisions_sorted() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path());
        save_decision(dir.path(), &sample_decision("D2")).unwrap();
        save_decision(dir.path(), &sample_decision("D1")).unwrap();
        let all = load_all_decisions(dir.path()).unwrap();
        let ids: Vec<&str> = all.iter().map(|d| d.decision_id.as_str()).collect();
        assert_eq!(ids, vec!["D1", "D2"]);
    }

    #[test]
    fn optional_fields_omitted_when_absent() {
        // A freshly proposed decision serializes without outcome/closed_at/
        // dissent/evidence_links keys (omitted, not null).
        let d = sample_decision("D1");
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("outcome"), "{json}");
        assert!(!json.contains("closed_at"), "{json}");
        assert!(!json.contains("dissent"), "{json}");
        assert!(!json.contains("evidence_links"), "{json}");
        // Required arrays are always present.
        assert!(json.contains("rounds"));
        assert!(json.contains("votes"));
    }
}
