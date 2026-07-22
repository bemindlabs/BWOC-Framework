//! `bwoc gws <verb>` — operator-facing CLI surface for the `gws` plugin kind
//! (BWOC-74). Foundation of `BWOC-EPIC-13` (Google Workspace, read-mostly).
//!
//! ## What this is
//!
//! The CLI half of the contract framed in
//! `notes/2026-05-28_google-workspace-plugin-architecture.md` (BWOC-72) and made
//! normative by the **Workspace Resource Schema** in `docs/en/PLUGINS.en.md`
//! (BWOC-73). It owns **argument parsing, workspace + plugin resolution, the
//! token-presence gate, pagination clamping, and the JSON shapes** — it does NOT
//! speak to Google directly. The live REST calls (Drive `files.list`, Gmail
//! `threads.list`, Calendar `events.list`, …) belong to the per-service
//! `gws`-kind plugins (`gws-drive`, `gws-gmail`, `gws-calendar`, BWOC-75/76), all
//! sourcing the `gws-auth` credential foundation. This CLI discovers each enabled
//! `gws-*` plugin by name + `kind = "gws"` and invokes its `[plugin].entry`; when
//! a plugin is absent the live verbs **stub-error gracefully** (exit `4`) rather
//! than panicking.
//!
//! ## Verb table
//!
//! | Verb                                  | Needs token | Plugin         | `operation` | Notes                                          |
//! |---|---|---|---|---|
//! | `auth status`                         | no          | `gws-auth`     | `status`    | Token present? granted scopes? account. Never the token value. |
//! | `drive list [--query] [--max]`        | yes         | `gws-drive`    | `list`      | Drive files in the Drive-file schema.          |
//! | `drive show --file <id>`              | yes         | `gws-drive`    | `get`       | One Drive file's metadata.                     |
//! | `gmail search [--query] [--max]`      | yes         | `gws-gmail`    | `search`    | Gmail threads in the Gmail-thread schema.      |
//! | `gmail show --thread <id>`            | yes         | `gws-gmail`    | `show`      | One thread (subject/from/labels/messages).     |
//! | `gmail labels`                        | yes         | `gws-gmail`    | `labels`    | Label list.                                    |
//! | `calendar list`                       | yes         | `gws-calendar` | `calendars` | Calendars the token can see.                   |
//! | `calendar events [--calendar] [--max]`| yes         | `gws-calendar` | `events`    | Events in the Calendar-event schema.           |
//! | `docs get --document <id>`            | yes         | `gws-docs`     | `get`       | Doc metadata + bounded body text.              |
//! | `docs batch-update --document <id> …` | yes         | `gws-docs`     | `batch-update` | **WRITE** (gated) — documents.batchUpdate.  |
//! | `docs replace-all-text --document <id> …` | yes     | `gws-docs`     | `replace-all-text` | **WRITE** (gated) — one replaceAllText.  |
//! | `sheets get --spreadsheet <id>`       | yes         | `gws-sheets`   | `get`       | Spreadsheet title + tab list.                  |
//! | `sheets values-get --spreadsheet <id> --range <a1>` | yes | `gws-sheets` | `values-get` | A value grid.                             |
//! | `sheets values-update --spreadsheet <id> --range <a1> …` | yes | `gws-sheets` | `values-update` | **WRITE** (gated) — overwrite a range. |
//! | `sheets values-append --spreadsheet <id> --range <a1> …` | yes | `gws-sheets` | `values-append` | **WRITE** (gated) — append rows.       |
//! | `slides get --presentation <id>`      | yes         | `gws-slides`   | `get`       | Presentation title + slide ids.                |
//! | `slides batch-update --presentation <id> …` | yes   | `gws-slides`   | `batch-update` | **WRITE** (gated) — presentations.batchUpdate. |
//! | `slides replace-all-text --presentation <id> …` | yes | `gws-slides` | `replace-all-text` | **WRITE** (gated) — one replaceAllText. |
//!
//! Every verb has a `--json` twin. The request payload is handed to the plugin
//! over **stdin as JSON** (the gcloud/jira dispatch precedent), carrying the
//! `operation` string above plus the verb's parameters; the plugin replies with
//! one JSON document on stdout.
//!
//! ## Auth model — operator OAuth token, never echoed
//!
//! Workspace REST authenticates with an **OAuth2 access token** (Bearer) carrying
//! user-consented readonly scopes. The token resolves from (precedence order, the
//! design-note pattern):
//!
//! 1. **`BWOC_GWS_TOKEN`** env — transient / CI;
//! 2. **`<workspace>/.bwoc/secrets/gws-token.json`** — workspace-local, gitignored.
//!
//! **This CLI only checks the token's *presence*** — it never reads, logs,
//! serializes, or forwards the value. The plugin (which inherits this process's
//! environment) reads `BWOC_GWS_TOKEN` / the secrets file itself and owns the
//! outbound `Authorization: Bearer` header and refresh. The read verbs
//! (`drive` / `gmail` / `calendar`) require a token and exit `2` when none is
//! present; `auth status` reports presence without requiring it (that is the point
//! of `status`). Mirrors the Adinnādāna invariant the `jira` / `gcloud` / `figma`
//! lanes established.
//!
//! ## Writes — the `docs` verbs, gated
//!
//! Drive / Gmail / Calendar stay read-only (send / insert / upload deferred).
//! `gws-docs` (BWOC-354) adds the first `gws` **write path** — `docs batch-update`
//! (documents.batchUpdate, the general write verb) and `docs replace-all-text`;
//! `gws-sheets` adds `sheets values-update` / `values-append`; `gws-slides` adds
//! `slides batch-update` / `replace-all-text`.
//! All carry the **operator-confirm gate** (PLUGINS §Write verbs): default No,
//! interactive `y/N`, `--yes` for headless agents, `--json` requires `--yes`, and
//! a refused write reports "no change" — the gate lives here at the CLI boundary,
//! not in the plugin. See `run_write_verb`.
//!
//! ## Pagination — `--max` caps an otherwise unbounded list
//!
//! The list verbs (`drive list`, `gmail search`, `calendar events`) page under the
//! hood in the plugin. `--max <n>` caps the total surfaced so an agent never pulls
//! an unbounded inbox; it is clamped to `1..=MAX_RESULTS_CEILING` before being
//! handed to the plugin. Omitting `--max` lets the plugin apply its own bounded
//! default.
//!
//! ## Exit codes — normative
//!
//! - `0` — success.
//! - `1` — local I/O error (e.g. JSON serialization).
//! - `2` — operator/usage error (no workspace, missing token for a read verb,
//!   malformed id).
//! - `4` — a required `gws-*` plugin is not enabled in this workspace (the live
//!   path is unavailable; the remediation message names the missing one).
//! - `255` — plugin runtime error (spawn failure or non-JSON output).
//!
//! Passing `--json` makes the exit code redundant: the structured envelope carries
//! `ok`/`error` fields with the same signal.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Exit codes + plugin names/kind + env var + paths (single source of truth).
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_PLUGIN: i32 = 4;
const EXIT_PLUGIN_ERROR: i32 = 255;

const PLUGIN_AUTH: &str = "gws-auth";
const PLUGIN_DRIVE: &str = "gws-drive";
const PLUGIN_GMAIL: &str = "gws-gmail";
const PLUGIN_CALENDAR: &str = "gws-calendar";
const PLUGIN_DOCS: &str = "gws-docs";
const PLUGIN_SHEETS: &str = "gws-sheets";
const PLUGIN_SLIDES: &str = "gws-slides";
const PLUGIN_KIND: &str = "gws";

const ENV_TOKEN: &str = "BWOC_GWS_TOKEN";
const SECRETS_REL: &str = ".bwoc/secrets/gws-token.json";

/// Upper bound `--max` is clamped to. The Workspace list endpoints page; this
/// keeps an agent from requesting an unbounded pull while still allowing a large
/// explicit page.
const MAX_RESULTS_CEILING: u32 = 1000;

// ---------------------------------------------------------------------------
// CLI surface — defined here so arg parsing is unit-testable against
// `GwsCommand` directly (see `tests` module).
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum GwsCommand {
    /// OAuth credential state operations (gws-auth plugin).
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Drive file operations (gws-drive plugin).
    #[command(subcommand)]
    Drive(DriveCommand),
    /// Gmail thread + label operations (gws-gmail plugin).
    #[command(subcommand)]
    Gmail(GmailCommand),
    /// Calendar + event operations (gws-calendar plugin).
    #[command(subcommand)]
    Calendar(CalendarCommand),
    /// Google Docs operations (gws-docs plugin) — read + in-place write.
    #[command(subcommand)]
    Docs(DocsCommand),
    /// Google Sheets operations (gws-sheets plugin) — read + values write.
    #[command(subcommand)]
    Sheets(SheetsCommand),
    /// Google Slides operations (gws-slides plugin) — read + in-place write.
    #[command(subcommand)]
    Slides(SlidesCommand),
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Report token presence, granted scopes, and account (never the token).
    Status(AuthStatusArgs),
}

#[derive(Subcommand, Debug)]
pub enum DriveCommand {
    /// List Drive files the token can see (Drive-file schema).
    List(DriveListArgs),
    /// Show one Drive file's metadata.
    Show(DriveShowArgs),
}

#[derive(Subcommand, Debug)]
pub enum GmailCommand {
    /// Search Gmail threads (Gmail-thread schema).
    Search(GmailSearchArgs),
    /// Show one thread (subject/from/labels + messages).
    Show(GmailShowArgs),
    /// List Gmail labels.
    Labels(GmailLabelsArgs),
}

#[derive(Subcommand, Debug)]
pub enum CalendarCommand {
    /// List calendars the token can see.
    List(CalendarListArgs),
    /// List events (Calendar-event schema).
    Events(CalendarEventsArgs),
}

#[derive(Subcommand, Debug)]
pub enum DocsCommand {
    /// Read a Google Doc's metadata + a bounded plain-text extract (documents.get).
    Get(DocsGetArgs),
    /// Edit a Doc in place via documents.batchUpdate — the general write path (gated).
    BatchUpdate(DocsBatchUpdateArgs),
    /// Replace every occurrence of a string in a Doc (gated write over replaceAllText).
    ReplaceAllText(DocsReplaceAllTextArgs),
}

#[derive(Subcommand, Debug)]
pub enum SheetsCommand {
    /// Read a spreadsheet's metadata + tab list (spreadsheets.get).
    Get(SheetsGetArgs),
    /// Read a cell range (spreadsheets.values.get).
    ValuesGet(SheetsValuesGetArgs),
    /// Overwrite a range's values (gated write — spreadsheets.values.update).
    ValuesUpdate(SheetsValuesWriteArgs),
    /// Append rows after a range (gated write — spreadsheets.values.append).
    ValuesAppend(SheetsValuesWriteArgs),
}

#[derive(Subcommand, Debug)]
pub enum SlidesCommand {
    /// Read a presentation's metadata + slide ids (presentations.get).
    Get(SlidesGetArgs),
    /// Edit a presentation via presentations.batchUpdate — the general write path (gated).
    BatchUpdate(SlidesBatchUpdateArgs),
    /// Replace every occurrence of a string in a presentation (gated write).
    ReplaceAllText(SlidesReplaceAllTextArgs),
}

#[derive(Args, Debug)]
pub struct AuthStatusArgs {
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DriveListArgs {
    /// Drive query (Drive `q` syntax, e.g. "mimeType='application/pdf'").
    #[arg(long)]
    query: Option<String>,
    /// Cap the number of files returned (clamped to 1..=1000).
    #[arg(long)]
    max: Option<u32>,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DriveShowArgs {
    /// Drive file id. Required.
    #[arg(long = "file")]
    file: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct GmailSearchArgs {
    /// Gmail search query (e.g. "from:me is:unread").
    #[arg(long)]
    query: Option<String>,
    /// Cap the number of threads returned (clamped to 1..=1000).
    #[arg(long)]
    max: Option<u32>,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct GmailShowArgs {
    /// Gmail thread id. Required.
    #[arg(long = "thread")]
    thread: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct GmailLabelsArgs {
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct CalendarListArgs {
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct CalendarEventsArgs {
    /// Calendar id to read events from (default: the token's primary calendar).
    #[arg(long = "calendar")]
    calendar: Option<String>,
    /// Cap the number of events returned (clamped to 1..=1000).
    #[arg(long)]
    max: Option<u32>,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DocsGetArgs {
    /// Google Doc document id. Required.
    #[arg(long = "document")]
    document: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("docs_requests").required(true).args(["requests", "requests_file"])))]
pub struct DocsBatchUpdateArgs {
    /// Google Doc document id. Required.
    #[arg(long = "document")]
    document: String,
    /// The Docs API `requests` array as an inline JSON string (write).
    #[arg(long = "requests")]
    requests: Option<String>,
    /// Path to a file holding the Docs API `requests` array as JSON (write).
    #[arg(long = "requests-file")]
    requests_file: Option<PathBuf>,
    /// Confirm the write without an interactive prompt (headless agents).
    #[arg(long)]
    yes: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DocsReplaceAllTextArgs {
    /// Google Doc document id. Required.
    #[arg(long = "document")]
    document: String,
    /// The text to match. Required.
    #[arg(long)]
    find: String,
    /// The replacement text (default: empty — deletes the matched text).
    #[arg(long, default_value = "")]
    replace: String,
    /// Match case when searching (default: case-insensitive).
    #[arg(long = "match-case")]
    match_case: bool,
    /// Confirm the write without an interactive prompt (headless agents).
    #[arg(long)]
    yes: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct SheetsGetArgs {
    /// Spreadsheet id. Required.
    #[arg(long = "spreadsheet")]
    spreadsheet: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct SheetsValuesGetArgs {
    /// Spreadsheet id. Required.
    #[arg(long = "spreadsheet")]
    spreadsheet: String,
    /// A1-notation range (e.g. `Sheet1!A1:B2`). Required.
    #[arg(long)]
    range: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("sheet_values").required(true).args(["values", "values_file"])))]
pub struct SheetsValuesWriteArgs {
    /// Spreadsheet id. Required.
    #[arg(long = "spreadsheet")]
    spreadsheet: String,
    /// A1-notation range (e.g. `Sheet1!A1`). Required.
    #[arg(long)]
    range: String,
    /// The values as an inline 2-D JSON array string (e.g. `[["a","b"],["c","d"]]`).
    #[arg(long = "values")]
    values: Option<String>,
    /// Path to a file holding the values as a 2-D JSON array.
    #[arg(long = "values-file")]
    values_file: Option<PathBuf>,
    /// Confirm the write without an interactive prompt (headless agents).
    #[arg(long)]
    yes: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct SlidesGetArgs {
    /// Presentation id. Required.
    #[arg(long = "presentation")]
    presentation: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("slides_requests").required(true).args(["requests", "requests_file"])))]
pub struct SlidesBatchUpdateArgs {
    /// Presentation id. Required.
    #[arg(long = "presentation")]
    presentation: String,
    /// The Slides API `requests` array as an inline JSON string (write).
    #[arg(long = "requests")]
    requests: Option<String>,
    /// Path to a file holding the Slides API `requests` array as JSON (write).
    #[arg(long = "requests-file")]
    requests_file: Option<PathBuf>,
    /// Confirm the write without an interactive prompt (headless agents).
    #[arg(long)]
    yes: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct SlidesReplaceAllTextArgs {
    /// Presentation id. Required.
    #[arg(long = "presentation")]
    presentation: String,
    /// The text to match. Required.
    #[arg(long)]
    find: String,
    /// The replacement text (default: empty — deletes the matched text).
    #[arg(long, default_value = "")]
    replace: String,
    /// Match case when searching (default: case-insensitive).
    #[arg(long = "match-case")]
    match_case: bool,
    /// Confirm the write without an interactive prompt (headless agents).
    #[arg(long)]
    yes: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

/// Dispatch a parsed `GwsCommand`. Returns the process exit code.
pub fn run(cmd: GwsCommand) -> i32 {
    match cmd {
        GwsCommand::Auth(AuthCommand::Status(a)) => run_auth_status(a),
        GwsCommand::Drive(DriveCommand::List(a)) => run_drive_list(a),
        GwsCommand::Drive(DriveCommand::Show(a)) => run_drive_show(a),
        GwsCommand::Gmail(GmailCommand::Search(a)) => run_gmail_search(a),
        GwsCommand::Gmail(GmailCommand::Show(a)) => run_gmail_show(a),
        GwsCommand::Gmail(GmailCommand::Labels(a)) => run_gmail_labels(a),
        GwsCommand::Calendar(CalendarCommand::List(a)) => run_calendar_list(a),
        GwsCommand::Calendar(CalendarCommand::Events(a)) => run_calendar_events(a),
        GwsCommand::Docs(DocsCommand::Get(a)) => run_docs_get(a),
        GwsCommand::Docs(DocsCommand::BatchUpdate(a)) => run_docs_batch_update(a),
        GwsCommand::Docs(DocsCommand::ReplaceAllText(a)) => run_docs_replace_all_text(a),
        GwsCommand::Sheets(SheetsCommand::Get(a)) => run_sheets_get(a),
        GwsCommand::Sheets(SheetsCommand::ValuesGet(a)) => run_sheets_values_get(a),
        GwsCommand::Sheets(SheetsCommand::ValuesUpdate(a)) => run_sheets_values_write(a, "update"),
        GwsCommand::Sheets(SheetsCommand::ValuesAppend(a)) => run_sheets_values_write(a, "append"),
        GwsCommand::Slides(SlidesCommand::Get(a)) => run_slides_get(a),
        GwsCommand::Slides(SlidesCommand::BatchUpdate(a)) => run_slides_batch_update(a),
        GwsCommand::Slides(SlidesCommand::ReplaceAllText(a)) => run_slides_replace_all_text(a),
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution — same shape as gcloud.rs / jira.rs / figma.rs.
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
// Auth shape — the token is NEVER captured. We surface presence + which source
// would win, derived from env + filesystem probes only. The `gws-auth` plugin's
// `status` verb returns the live answer (granted scopes, account); this is the
// offline pre-check that gates the read verbs and feeds the remediation message.
// ---------------------------------------------------------------------------

/// Where an OAuth token would resolve from. `gws-auth status` returns the live
/// answer; this is the offline pre-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TokenSource {
    Env,
    SecretsFile,
    None,
}

impl TokenSource {
    fn as_str(self) -> &'static str {
        match self {
            TokenSource::Env => "env",
            TokenSource::SecretsFile => "secrets-file",
            TokenSource::None => "none",
        }
    }
}

/// Offline token probe. Env presence + secrets-file presence only — the token
/// value is never read, hashed, or surfaced.
#[derive(Debug, Clone, Serialize, PartialEq)]
struct AuthShape {
    /// First source that would resolve, per the precedence in the design note.
    active_source: TokenSource,
    /// Whether `BWOC_GWS_TOKEN` is set (non-empty).
    env_token_present: bool,
    /// Whether `<workspace>/.bwoc/secrets/gws-token.json` exists. Presence only —
    /// the file is never read or hashed here.
    secrets_file_present: bool,
}

impl AuthShape {
    /// True when any token source is present (the read-verb gate).
    fn has_token(&self) -> bool {
        self.active_source != TokenSource::None
    }
}

fn probe_auth_shape(workspace: &Path, getenv: &dyn Fn(&str) -> Option<String>) -> AuthShape {
    let env_token_present = getenv(ENV_TOKEN).filter(|s| !s.is_empty()).is_some();
    let secrets_file_present = workspace.join(SECRETS_REL).is_file();

    // Precedence: env > secrets file.
    let active_source = if env_token_present {
        TokenSource::Env
    } else if secrets_file_present {
        TokenSource::SecretsFile
    } else {
        TokenSource::None
    };

    AuthShape {
        active_source,
        env_token_present,
        secrets_file_present,
    }
}

fn real_getenv(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

// ---------------------------------------------------------------------------
// Pagination — clamp `--max` to a sane window before handing it to the plugin.
// ---------------------------------------------------------------------------

/// Clamp an explicit `--max` to `1..=MAX_RESULTS_CEILING`. `None` (no `--max`)
/// stays `None` so the plugin applies its own bounded default. `Some(0)` clamps
/// up to `1` — a zero-result page is never what the operator meant.
fn normalize_max(max: Option<u32>) -> Option<u32> {
    max.map(|n| n.clamp(1, MAX_RESULTS_CEILING))
}

// ---------------------------------------------------------------------------
// Id validation — local pre-check. Values travel to the plugin over JSON stdin
// (not argv), so there is no CLI→plugin option-injection surface; the guards
// reject empty / `-`-leading / over-long / out-of-charset junk before we spawn.
// ---------------------------------------------------------------------------

/// Drive file id / Gmail thread id: Google opaque ids — letters, digits, `_`,
/// `-`. 1..=512 chars, no leading hyphen.
fn is_valid_resource_id(id: &str) -> bool {
    let b = id.as_bytes();
    if !(1..=512).contains(&b.len()) {
        return false;
    }
    if b[0] == b'-' {
        return false;
    }
    b.iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// Calendar id: opaque id, the literal `primary`, or an email-like address
/// (e.g. `…@group.calendar.google.com`). Adds `.` and `@` to the charset;
/// 1..=512 chars, no leading hyphen.
fn is_valid_calendar_id(id: &str) -> bool {
    let b = id.as_bytes();
    if !(1..=512).contains(&b.len()) {
        return false;
    }
    if b[0] == b'-' {
        return false;
    }
    b.iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' || c == b'@')
}

/// A1-notation range (Sheets): letters, digits, `_`, `!`, `:`, `$`, `'`, `.`,
/// space. 1..=512 chars, no `/` and no control bytes — enough for `Sheet1!A1:B2`
/// / `'My Sheet'!A:A` without opening a path segment in the request URL.
fn is_valid_range(r: &str) -> bool {
    if !(1..=512).contains(&r.len()) {
        return false;
    }
    r.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '!' | ':' | '$' | '\'' | '.' | ' ' | '-')
    })
}

/// Free-text query (Drive `q`, Gmail search). 1..=1024 chars, no control bytes
/// (tabs/newlines excluded — they have no meaning in a single-line query and a
/// stray control byte usually signals a paste error or injection attempt).
fn is_valid_query(q: &str) -> bool {
    let len = q.len();
    if !(1..=1024).contains(&len) {
        return false;
    }
    !q.chars().any(|c| c.is_control())
}

// ---------------------------------------------------------------------------
// Plugin discovery — finds the enabled `gws-*` plugin by name + kind=gws.
// Mirrors gcloud.rs exactly: checks both the flat layout
// (`modules/plugins/<name>/`) and the kind-namespaced layout
// (`modules/plugins/gws/<name>/`).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct ManifestRaw {
    plugin: PluginSection,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PluginSection {
    name: String,
    kind: String,
    entry: String,
}

#[derive(Debug, Clone, PartialEq)]
struct GwsPlugin {
    name: String,
    dir: PathBuf,
    entry: String,
}

/// Read `.bwoc/workspace.toml [plugins.<name>] enabled` flags.
fn workspace_enabled_set(root: &Path) -> Result<BTreeMap<String, bool>, String> {
    let path = root.join(".bwoc/workspace.toml");
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&body).map_err(|e| format!("{}: parse: {e}", path.display()))?;
    let mut out = BTreeMap::new();
    let Some(plugins) = value.get("plugins").and_then(|v| v.as_table()) else {
        return Ok(out);
    };
    for (name, entry) in plugins {
        let enabled = entry
            .as_table()
            .and_then(|t| t.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.insert(name.clone(), enabled);
    }
    Ok(out)
}

/// Try the two known plugin layouts in order — flat, then `gws/`-namespaced.
fn candidate_plugin_dirs(root: &Path, name: &str) -> [PathBuf; 2] {
    [
        root.join("modules/plugins").join(name),
        root.join("modules/plugins/gws").join(name),
    ]
}

/// Find a `gws`-kind plugin by name across both layouts. Returns `None` when no
/// manifest matches; returns `Err` on parse failure (the plugin *exists* but is
/// malformed — surface, don't silently degrade).
fn discover_plugin(root: &Path, name: &str) -> Result<Option<GwsPlugin>, String> {
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
            // Wrong manifest at this path — keep looking.
            continue;
        }
        if parsed.plugin.kind != PLUGIN_KIND {
            // Right name, wrong kind. Surface — this is a misconfiguration.
            return Err(format!(
                "{}: [plugin].kind = {:?}, expected {:?}",
                manifest.display(),
                parsed.plugin.kind,
                PLUGIN_KIND
            ));
        }
        return Ok(Some(GwsPlugin {
            name: parsed.plugin.name,
            dir: plugin_dir,
            entry: parsed.plugin.entry,
        }));
    }
    Ok(None)
}

/// Discover + check the `enabled` flag in `workspace.toml`. A plugin installed
/// but disabled returns `None` — same stub-error path as "not installed".
fn find_enabled_plugin(root: &Path, name: &str) -> Result<Option<GwsPlugin>, String> {
    let Some(plugin) = discover_plugin(root, name)? else {
        return Ok(None);
    };
    let enabled = workspace_enabled_set(root)?;
    if matches!(enabled.get(name), Some(true)) {
        Ok(Some(plugin))
    } else {
        Ok(None)
    }
}

fn resolve_entry_program(plugin_dir: &Path, entry: &str) -> OsString {
    let candidate = plugin_dir.join(entry);
    if candidate.is_file() {
        candidate.into_os_string()
    } else {
        OsString::from(entry)
    }
}

// ---------------------------------------------------------------------------
// Plugin invocation — same shape as gcloud.rs::invoke_plugin. The token is NOT
// passed explicitly: the plugin inherits this process's environment (including
// `BWOC_GWS_TOKEN`) and reads it itself. We never touch the value.
// ---------------------------------------------------------------------------

fn invoke_plugin(
    plugin: &GwsPlugin,
    workspace: &Path,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    // BWOC-36: guard against path-traversal RCE before spawning the entry.
    crate::util::validate_plugin_entry(&plugin.entry)?;
    let program = resolve_entry_program(&plugin.dir, &plugin.entry);
    let operation = request
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut child = Command::new(&program)
        .current_dir(&plugin.dir)
        .env("BWOC_WORKSPACE", workspace)
        .env("BWOC_PLUGIN_DIR", &plugin.dir)
        .env("BWOC_GWS_OPERATION", operation)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn '{}': {e}", program.to_string_lossy()))?;

    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "{request}");
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .map_err(|e| format!("wait '{}': {e}", program.to_string_lossy()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "plugin '{}' exited {} (stderr: {})",
            plugin.name,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| format!("plugin '{}' did not emit valid JSON: {e}", plugin.name))
}

// ---------------------------------------------------------------------------
// Request payloads handed to the plugin over stdin (one per verb). Optional
// params serialize as JSON null (present-but-absent), per the gcloud precedent.
// ---------------------------------------------------------------------------

fn auth_status_request(workspace: &Path, plugin_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "operation": "status",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
    })
}

fn drive_list_request(
    workspace: &Path,
    plugin_dir: &Path,
    query: Option<&str>,
    max: Option<u32>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "list",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "query": query,
        "max": max,
    })
}

fn drive_show_request(workspace: &Path, plugin_dir: &Path, file_id: &str) -> serde_json::Value {
    serde_json::json!({
        "operation": "get",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "file_id": file_id,
    })
}

fn gmail_search_request(
    workspace: &Path,
    plugin_dir: &Path,
    query: Option<&str>,
    max: Option<u32>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "search",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "query": query,
        "max": max,
    })
}

fn gmail_show_request(workspace: &Path, plugin_dir: &Path, thread_id: &str) -> serde_json::Value {
    serde_json::json!({
        "operation": "show",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "thread_id": thread_id,
    })
}

fn gmail_labels_request(workspace: &Path, plugin_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "operation": "labels",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
    })
}

fn calendar_list_request(workspace: &Path, plugin_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "operation": "calendars",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
    })
}

fn calendar_events_request(
    workspace: &Path,
    plugin_dir: &Path,
    calendar: Option<&str>,
    max: Option<u32>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "events",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "calendar_id": calendar,
        "max": max,
    })
}

fn docs_get_request(workspace: &Path, plugin_dir: &Path, document_id: &str) -> serde_json::Value {
    serde_json::json!({
        "operation": "get",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "document_id": document_id,
    })
}

fn docs_batch_update_request(
    workspace: &Path,
    plugin_dir: &Path,
    document_id: &str,
    requests: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "batch-update",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "document_id": document_id,
        "requests": requests,
    })
}

fn docs_replace_all_text_request(
    workspace: &Path,
    plugin_dir: &Path,
    document_id: &str,
    find: &str,
    replace: &str,
    match_case: bool,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "replace-all-text",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "document_id": document_id,
        "find": find,
        "replace": replace,
        "match_case": match_case,
    })
}

fn sheets_get_request(
    workspace: &Path,
    plugin_dir: &Path,
    spreadsheet_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "get",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "spreadsheet_id": spreadsheet_id,
    })
}

fn sheets_values_get_request(
    workspace: &Path,
    plugin_dir: &Path,
    spreadsheet_id: &str,
    range: &str,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "values-get",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "spreadsheet_id": spreadsheet_id,
        "range": range,
    })
}

fn sheets_values_write_request(
    workspace: &Path,
    plugin_dir: &Path,
    mode: &str,
    spreadsheet_id: &str,
    range: &str,
    values: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "operation": format!("values-{mode}"),
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "spreadsheet_id": spreadsheet_id,
        "range": range,
        "values": values,
    })
}

fn slides_get_request(
    workspace: &Path,
    plugin_dir: &Path,
    presentation_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "get",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "presentation_id": presentation_id,
    })
}

fn slides_batch_update_request(
    workspace: &Path,
    plugin_dir: &Path,
    presentation_id: &str,
    requests: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "batch-update",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "presentation_id": presentation_id,
        "requests": requests,
    })
}

fn slides_replace_all_text_request(
    workspace: &Path,
    plugin_dir: &Path,
    presentation_id: &str,
    find: &str,
    replace: &str,
    match_case: bool,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "replace-all-text",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "presentation_id": presentation_id,
        "find": find,
        "replace": replace,
        "match_case": match_case,
    })
}

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

fn print_json(value: &serde_json::Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(e) => {
            eprintln!("bwoc gws: serialize JSON: {e}");
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

/// Stub-error envelope for the missing-plugin path. Names the exact plugin and
/// the install hint the operator needs.
fn no_plugin_message(plugin_name: &str) -> String {
    format!(
        "no enabled '{plugin_name}' plugin (gws kind) in this workspace. \
         The live Google Workspace path is provided by `{plugin_name}` (see the \
         EPIC-13 design note). Install it (BWOC-75/76) with \
         `bwoc plugin install <source>` then `bwoc plugin enable {plugin_name}`."
    )
}

fn require_plugin(
    root: &Path,
    plugin_name: &str,
    verb: &str,
    json: bool,
) -> Result<GwsPlugin, i32> {
    match find_enabled_plugin(root, plugin_name) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => {
            let msg = no_plugin_message(plugin_name);
            if json {
                emit_error_json(verb, "no_plugin", &msg);
            } else {
                eprintln!("bwoc gws {verb}: {msg}");
            }
            Err(EXIT_NO_PLUGIN)
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "discovery_error", &e);
            } else {
                eprintln!("bwoc gws {verb}: {e}");
            }
            Err(EXIT_PLUGIN_ERROR)
        }
    }
}

/// The read-verb token gate. Returns `Ok(())` when a token is present; otherwise
/// emits the usage error and the exit code to return. `auth status` skips this.
fn require_token(shape: &AuthShape, verb: &str, json: bool) -> Result<(), i32> {
    if shape.has_token() {
        return Ok(());
    }
    let msg = format!(
        "no OAuth token found. Set {ENV_TOKEN} or create {SECRETS_REL} (gitignored). \
         Run `bwoc gws auth status` to inspect credential state."
    );
    if json {
        emit_error_json(verb, "no_token", &msg);
    } else {
        eprintln!("bwoc gws {verb}: {msg}");
    }
    Err(EXIT_USAGE)
}

/// Resolve the workspace, printing the usage error under `verb` on failure.
fn workspace_or_usage(workspace: Option<PathBuf>, verb: &str) -> Result<PathBuf, i32> {
    resolve_workspace(workspace).map_err(|e| {
        eprintln!("bwoc gws {verb}: {e}");
        EXIT_USAGE
    })
}

/// Run a read verb that needs a token + an enabled plugin, then relay the
/// plugin's JSON. `render` prints the human-readable view from the plugin value.
fn run_read_verb(
    verb: &str,
    plugin_name: &str,
    workspace: Option<PathBuf>,
    json: bool,
    build_request: impl FnOnce(&Path, &Path) -> serde_json::Value,
    render: impl FnOnce(&serde_json::Value),
) -> i32 {
    let root = match workspace_or_usage(workspace, verb) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let shape = probe_auth_shape(&root, &real_getenv);
    if let Err(code) = require_token(&shape, verb, json) {
        return code;
    }
    let plugin = match require_plugin(&root, plugin_name, verb, json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = build_request(&root, &plugin.dir);
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_LOCAL_ERROR
                }
            } else {
                render(&value);
                EXIT_OK
            }
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "plugin_error", &e);
            } else {
                eprintln!("bwoc gws {verb}: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// Write-verb operator-confirm gate (PLUGINS §Write verbs). The gate lives at the
// operator boundary — this CLI — not the plugin. Default is No; a headless agent
// passes `--yes`; `--json` requires `--yes` (a non-interactive context cannot
// prompt). A refused write reports "no change", never a bare failure.
// ---------------------------------------------------------------------------

/// A gated write requested in `--json` mode cannot prompt — it requires `--yes`.
fn json_write_blocked(json: bool, yes: bool) -> bool {
    json && !yes
}

/// Interactive y/N confirmation on stderr. EOF / anything but yes → false.
fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Run a write verb: token gate → operator-confirm gate → enabled plugin →
/// relay the plugin's JSON write receipt. `prompt` is shown for the interactive
/// confirmation; `--yes` skips it. A refused write returns `EXIT_USAGE` after
/// reporting "no change".
#[allow(clippy::too_many_arguments)]
fn run_write_verb(
    verb: &str,
    plugin_name: &str,
    workspace: Option<PathBuf>,
    json: bool,
    yes: bool,
    prompt: String,
    build_request: impl FnOnce(&Path, &Path) -> serde_json::Value,
    render: impl FnOnce(&serde_json::Value),
) -> i32 {
    let root = match workspace_or_usage(workspace, verb) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let shape = probe_auth_shape(&root, &real_getenv);
    if let Err(code) = require_token(&shape, verb, json) {
        return code;
    }
    // The gate — one confirmation point per write.
    if !yes {
        if json_write_blocked(json, yes) {
            eprintln!("bwoc gws {verb}: --json requires --yes (a write needs explicit ack)");
            return EXIT_USAGE;
        }
        if !confirm(&prompt) {
            eprintln!("bwoc gws {verb}: aborted — no change performed.");
            return EXIT_USAGE;
        }
    }
    let plugin = match require_plugin(&root, plugin_name, verb, json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = build_request(&root, &plugin.dir);
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_LOCAL_ERROR
                }
            } else {
                render(&value);
                EXIT_OK
            }
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "plugin_error", &e);
            } else {
                eprintln!("bwoc gws {verb}: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

/// Accept either a `{ "<key>": [...] }` envelope or a bare top-level array.
fn array_under<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a Vec<serde_json::Value>> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array())
}

fn field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("?")
}

// ---------------------------------------------------------------------------
// Verb implementations.
// ---------------------------------------------------------------------------

fn run_auth_status(args: AuthStatusArgs) -> i32 {
    let verb = "auth status";
    let root = match workspace_or_usage(args.workspace, verb) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let shape = probe_auth_shape(&root, &real_getenv);

    // `auth status` does not require a token — reporting its absence is the point.
    let plugin = match require_plugin(&root, PLUGIN_AUTH, verb, args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = auth_status_request(&root, &plugin.dir);
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                let merged = serde_json::json!({
                    "ok": true,
                    "workspace": root.display().to_string(),
                    "auth": value,
                    "shape": shape,
                });
                if print_json(&merged) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let account = field(&value, "account");
                let has = value
                    .get("has_credential")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(shape.has_token());
                let scopes = value
                    .get("scopes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "(unknown)".to_string());
                println!(
                    "bwoc gws auth: source={}, account={account}, has_credential={has}, scopes=[{scopes}]",
                    shape.active_source.as_str()
                );
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json(verb, "plugin_error", &e);
            } else {
                eprintln!("bwoc gws {verb}: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

fn run_drive_list(args: DriveListArgs) -> i32 {
    let verb = "drive list";
    if let Some(q) = &args.query {
        if !is_valid_query(q) {
            return usage_bad_field(
                verb,
                "bad_query",
                "query must be 1..=1024 chars, no control bytes",
                args.json,
            );
        }
    }
    let max = normalize_max(args.max);
    let query = args.query.clone();
    run_read_verb(
        verb,
        PLUGIN_DRIVE,
        args.workspace,
        args.json,
        move |ws, dir| drive_list_request(ws, dir, query.as_deref(), max),
        |value| {
            let files = array_under(value, "files");
            let total = files.map(|a| a.len()).unwrap_or(0);
            println!("bwoc gws drive list: {total} file(s)");
            if let Some(arr) = files {
                for f in arr {
                    println!(
                        "  {}  {} [{}] {}",
                        field(f, "file_id"),
                        field(f, "name"),
                        field(f, "mime_type"),
                        field(f, "modified_time"),
                    );
                }
            }
        },
    )
}

fn run_drive_show(args: DriveShowArgs) -> i32 {
    let verb = "drive show";
    if !is_valid_resource_id(&args.file) {
        return usage_bad_field(
            verb,
            "bad_file_id",
            "file id must be 1..=512 chars of [A-Za-z0-9_-], no leading hyphen",
            args.json,
        );
    }
    let file = args.file.clone();
    run_read_verb(
        verb,
        PLUGIN_DRIVE,
        args.workspace,
        args.json,
        move |ws, dir| drive_show_request(ws, dir, &file),
        |value| {
            println!(
                "bwoc gws drive show: {} — {} [{}] {}",
                field(value, "file_id"),
                field(value, "name"),
                field(value, "mime_type"),
                field(value, "modified_time"),
            );
        },
    )
}

fn run_gmail_search(args: GmailSearchArgs) -> i32 {
    let verb = "gmail search";
    if let Some(q) = &args.query {
        if !is_valid_query(q) {
            return usage_bad_field(
                verb,
                "bad_query",
                "query must be 1..=1024 chars, no control bytes",
                args.json,
            );
        }
    }
    let max = normalize_max(args.max);
    let query = args.query.clone();
    run_read_verb(
        verb,
        PLUGIN_GMAIL,
        args.workspace,
        args.json,
        move |ws, dir| gmail_search_request(ws, dir, query.as_deref(), max),
        |value| {
            let threads = array_under(value, "threads");
            let total = threads.map(|a| a.len()).unwrap_or(0);
            println!("bwoc gws gmail search: {total} thread(s)");
            if let Some(arr) = threads {
                for t in arr {
                    println!(
                        "  {}  {} — {} ({})",
                        field(t, "thread_id"),
                        field(t, "subject"),
                        field(t, "from"),
                        field(t, "last_message_time"),
                    );
                }
            }
        },
    )
}

fn run_gmail_show(args: GmailShowArgs) -> i32 {
    let verb = "gmail show";
    if !is_valid_resource_id(&args.thread) {
        return usage_bad_field(
            verb,
            "bad_thread_id",
            "thread id must be 1..=512 chars of [A-Za-z0-9_-], no leading hyphen",
            args.json,
        );
    }
    let thread = args.thread.clone();
    run_read_verb(
        verb,
        PLUGIN_GMAIL,
        args.workspace,
        args.json,
        move |ws, dir| gmail_show_request(ws, dir, &thread),
        |value| {
            let labels = value
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            println!(
                "bwoc gws gmail show: {} — {} (from {}) labels=[{labels}]",
                field(value, "thread_id"),
                field(value, "subject"),
                field(value, "from"),
            );
        },
    )
}

fn run_gmail_labels(args: GmailLabelsArgs) -> i32 {
    let verb = "gmail labels";
    run_read_verb(
        verb,
        PLUGIN_GMAIL,
        args.workspace,
        args.json,
        gmail_labels_request,
        |value| {
            let labels = array_under(value, "labels");
            let total = labels.map(|a| a.len()).unwrap_or(0);
            println!("bwoc gws gmail labels: {total} label(s)");
            if let Some(arr) = labels {
                for l in arr {
                    // Accept a bare string or an object with name/id.
                    if let Some(s) = l.as_str() {
                        println!("  {s}");
                    } else {
                        println!("  {}", field(l, "name"));
                    }
                }
            }
        },
    )
}

fn run_calendar_list(args: CalendarListArgs) -> i32 {
    let verb = "calendar list";
    run_read_verb(
        verb,
        PLUGIN_CALENDAR,
        args.workspace,
        args.json,
        calendar_list_request,
        |value| {
            let cals = array_under(value, "calendars");
            let total = cals.map(|a| a.len()).unwrap_or(0);
            println!("bwoc gws calendar list: {total} calendar(s)");
            if let Some(arr) = cals {
                for c in arr {
                    println!("  {}  {}", field(c, "calendar_id"), field(c, "summary"));
                }
            }
        },
    )
}

fn run_calendar_events(args: CalendarEventsArgs) -> i32 {
    let verb = "calendar events";
    if let Some(c) = &args.calendar {
        if !is_valid_calendar_id(c) {
            return usage_bad_field(
                verb,
                "bad_calendar_id",
                "calendar id must be 1..=512 chars of [A-Za-z0-9_-.@], no leading hyphen",
                args.json,
            );
        }
    }
    let max = normalize_max(args.max);
    let calendar = args.calendar.clone();
    run_read_verb(
        verb,
        PLUGIN_CALENDAR,
        args.workspace,
        args.json,
        move |ws, dir| calendar_events_request(ws, dir, calendar.as_deref(), max),
        |value| {
            let events = array_under(value, "events");
            let total = events.map(|a| a.len()).unwrap_or(0);
            println!("bwoc gws calendar events: {total} event(s)");
            if let Some(arr) = events {
                for e in arr {
                    println!(
                        "  {}  {} ({} → {})",
                        field(e, "event_id"),
                        field(e, "summary"),
                        field(e, "start"),
                        field(e, "end"),
                    );
                }
            }
        },
    )
}

/// Emit a usage error for a malformed field and return `EXIT_USAGE`.
fn usage_bad_field(verb: &str, code: &str, message: &str, json: bool) -> i32 {
    if json {
        emit_error_json(verb, code, message);
    } else {
        eprintln!("bwoc gws {verb}: {message}");
    }
    EXIT_USAGE
}

const BAD_DOCUMENT_ID: &str =
    "document id must be 1..=512 chars of [A-Za-z0-9_-], no leading hyphen";

fn run_docs_get(args: DocsGetArgs) -> i32 {
    let verb = "docs get";
    if !is_valid_resource_id(&args.document) {
        return usage_bad_field(verb, "bad_document_id", BAD_DOCUMENT_ID, args.json);
    }
    let document = args.document.clone();
    run_read_verb(
        verb,
        PLUGIN_DOCS,
        args.workspace,
        args.json,
        move |ws, dir| docs_get_request(ws, dir, &document),
        |value| {
            let d = value.get("document").unwrap_or(value);
            println!("bwoc gws docs get: {}", field(d, "title"));
            println!("  document_id: {}", field(d, "document_id"));
            println!("  revision_id: {}", field(d, "revision_id"));
            if let Some(link) = d.get("web_view_link").and_then(|v| v.as_str()) {
                println!("  {link}");
            }
            if let Some(t) = value.get("text").and_then(|v| v.as_str()) {
                let more = value
                    .get("text_truncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    || t.chars().count() > 280;
                let preview: String = t.chars().take(280).collect();
                println!("  ── body ──\n{preview}{}", if more { " …" } else { "" });
            }
        },
    )
}

/// Resolve the `requests` JSON array from `--requests` (inline) or
/// `--requests-file` (path). Validates it is a non-empty JSON array.
fn resolve_docs_requests(
    inline: Option<&str>,
    file: Option<&Path>,
    verb: &str,
    json: bool,
) -> Result<serde_json::Value, i32> {
    // Name the actual source in every diagnostic — the JSON came from one of them.
    let (raw, source) = if let Some(path) = file {
        match std::fs::read_to_string(path) {
            Ok(s) => (s, "--requests-file"),
            Err(e) => {
                return Err(usage_bad_field(
                    verb,
                    "requests_file_read",
                    &format!("read {}: {e}", path.display()),
                    json,
                ));
            }
        }
    } else if let Some(s) = inline {
        (s.to_string(), "--requests")
    } else {
        return Err(usage_bad_field(
            verb,
            "no_requests",
            "one of --requests or --requests-file is required",
            json,
        ));
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return Err(usage_bad_field(
                verb,
                "requests_parse",
                &format!("{source} is not valid JSON: {e}"),
                json,
            ));
        }
    };
    match value.as_array() {
        Some(a) if !a.is_empty() => Ok(value),
        Some(_) => Err(usage_bad_field(
            verb,
            "requests_empty",
            &format!("{source} is an empty array — nothing to apply"),
            json,
        )),
        None => Err(usage_bad_field(
            verb,
            "requests_not_array",
            &format!("{source} must be a JSON array of Docs API request objects"),
            json,
        )),
    }
}

fn run_docs_batch_update(args: DocsBatchUpdateArgs) -> i32 {
    let verb = "docs batch-update";
    if !is_valid_resource_id(&args.document) {
        return usage_bad_field(verb, "bad_document_id", BAD_DOCUMENT_ID, args.json);
    }
    let requests = match resolve_docs_requests(
        args.requests.as_deref(),
        args.requests_file.as_deref(),
        verb,
        args.json,
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let n = requests.as_array().map(|a| a.len()).unwrap_or(0);
    let document = args.document.clone();
    let prompt = format!(
        "Apply {n} batchUpdate request(s) to Google Doc {document}? This edits the live document."
    );
    run_write_verb(
        verb,
        PLUGIN_DOCS,
        args.workspace,
        args.json,
        args.yes,
        prompt,
        move |ws, dir| docs_batch_update_request(ws, dir, &document, &requests),
        render_docs_write,
    )
}

fn run_docs_replace_all_text(args: DocsReplaceAllTextArgs) -> i32 {
    let verb = "docs replace-all-text";
    if !is_valid_resource_id(&args.document) {
        return usage_bad_field(verb, "bad_document_id", BAD_DOCUMENT_ID, args.json);
    }
    if !(1..=1024).contains(&args.find.len()) {
        return usage_bad_field(verb, "bad_find", "--find must be 1..=1024 bytes", args.json);
    }
    let document = args.document.clone();
    let find = args.find.clone();
    let replace = args.replace.clone();
    let match_case = args.match_case;
    let prompt = format!(
        "Replace all '{find}' → '{replace}' in Google Doc {document}? This edits the live document."
    );
    run_write_verb(
        verb,
        PLUGIN_DOCS,
        args.workspace,
        args.json,
        args.yes,
        prompt,
        move |ws, dir| {
            docs_replace_all_text_request(ws, dir, &document, &find, &replace, match_case)
        },
        render_docs_write,
    )
}

/// Human-readable write receipt for `batch-update` / `replace-all-text`.
fn render_docs_write(value: &serde_json::Value) {
    println!(
        "bwoc gws {}: applied to Google Doc {}",
        field(value, "operation"),
        field(value, "document_id"),
    );
    if let Some(n) = value.get("requests_applied").and_then(|v| v.as_i64()) {
        println!("  requests applied: {n}");
    }
    if let Some(occ) = value.get("occurrences_changed").and_then(|v| v.as_i64()) {
        println!("  occurrences changed: {occ}");
    }
    if let Some(rev) = value.get("revision_id").and_then(|v| v.as_str()) {
        if !rev.is_empty() {
            println!("  revision: {rev}");
        }
    }
}

const BAD_SPREADSHEET_ID: &str =
    "spreadsheet id must be 1..=512 chars of [A-Za-z0-9_-], no leading hyphen";
const BAD_RANGE: &str = "range must be A1 notation (e.g. Sheet1!A1:B2), 1..=512 chars, no '/'";

fn run_sheets_get(args: SheetsGetArgs) -> i32 {
    let verb = "sheets get";
    if !is_valid_resource_id(&args.spreadsheet) {
        return usage_bad_field(verb, "bad_spreadsheet_id", BAD_SPREADSHEET_ID, args.json);
    }
    let id = args.spreadsheet.clone();
    run_read_verb(
        verb,
        PLUGIN_SHEETS,
        args.workspace,
        args.json,
        move |ws, dir| sheets_get_request(ws, dir, &id),
        |value| {
            let s = value.get("spreadsheet").unwrap_or(value);
            println!("bwoc gws sheets get: {}", field(s, "title"));
            println!("  spreadsheet_id: {}", field(s, "spreadsheet_id"));
            if let Some(link) = s.get("web_view_link").and_then(|v| v.as_str()) {
                println!("  {link}");
            }
            if let Some(arr) = value.get("sheets").and_then(|v| v.as_array()) {
                println!("  tabs: {}", arr.len());
                for t in arr {
                    println!("    [{}] {}", field(t, "index"), field(t, "title"));
                }
            }
        },
    )
}

fn run_sheets_values_get(args: SheetsValuesGetArgs) -> i32 {
    let verb = "sheets values-get";
    if !is_valid_resource_id(&args.spreadsheet) {
        return usage_bad_field(verb, "bad_spreadsheet_id", BAD_SPREADSHEET_ID, args.json);
    }
    if !is_valid_range(&args.range) {
        return usage_bad_field(verb, "bad_range", BAD_RANGE, args.json);
    }
    let id = args.spreadsheet.clone();
    let range = args.range.clone();
    run_read_verb(
        verb,
        PLUGIN_SHEETS,
        args.workspace,
        args.json,
        move |ws, dir| sheets_values_get_request(ws, dir, &id, &range),
        |value| {
            println!("bwoc gws sheets values-get: {} ", field(value, "range"));
            if let Some(rows) = value.get("values").and_then(|v| v.as_array()) {
                println!("  {} row(s)", rows.len());
                for row in rows.iter().take(20) {
                    if let Some(cells) = row.as_array() {
                        let joined: Vec<String> = cells
                            .iter()
                            .map(|c| {
                                c.as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| c.to_string())
                            })
                            .collect();
                        println!("    {}", joined.join(" | "));
                    }
                }
                if rows.len() > 20 {
                    println!("    … ({} more rows)", rows.len() - 20);
                }
            }
        },
    )
}

/// Resolve `values` from `--values` (inline) or `--values-file` (path).
/// Validates it is a 2-D JSON array (array of row arrays).
fn resolve_sheet_values(
    inline: Option<&str>,
    file: Option<&Path>,
    verb: &str,
    json: bool,
) -> Result<serde_json::Value, i32> {
    let (raw, source) = if let Some(path) = file {
        match std::fs::read_to_string(path) {
            Ok(s) => (s, "--values-file"),
            Err(e) => {
                return Err(usage_bad_field(
                    verb,
                    "values_file_read",
                    &format!("read {}: {e}", path.display()),
                    json,
                ));
            }
        }
    } else if let Some(s) = inline {
        (s.to_string(), "--values")
    } else {
        return Err(usage_bad_field(
            verb,
            "no_values",
            "one of --values or --values-file is required",
            json,
        ));
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return Err(usage_bad_field(
                verb,
                "values_parse",
                &format!("{source} is not valid JSON: {e}"),
                json,
            ));
        }
    };
    // Must be a NON-EMPTY array of arrays. `iter().all(..)` is vacuously true on
    // an empty outer array, so guard `!is_empty()` explicitly — an empty value
    // set is a no-op write, never what the operator meant.
    let is_2d = value
        .as_array()
        .is_some_and(|rows| !rows.is_empty() && rows.iter().all(|r| r.is_array()));
    if is_2d {
        Ok(value)
    } else {
        Err(usage_bad_field(
            verb,
            "values_not_2d",
            &format!("{source} must be a non-empty 2-D JSON array (array of row arrays)"),
            json,
        ))
    }
}

fn run_sheets_values_write(args: SheetsValuesWriteArgs, mode: &str) -> i32 {
    let verb = if mode == "append" {
        "sheets values-append"
    } else {
        "sheets values-update"
    };
    if !is_valid_resource_id(&args.spreadsheet) {
        return usage_bad_field(verb, "bad_spreadsheet_id", BAD_SPREADSHEET_ID, args.json);
    }
    if !is_valid_range(&args.range) {
        return usage_bad_field(verb, "bad_range", BAD_RANGE, args.json);
    }
    let values = match resolve_sheet_values(
        args.values.as_deref(),
        args.values_file.as_deref(),
        verb,
        args.json,
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rows = values.as_array().map(|a| a.len()).unwrap_or(0);
    let id = args.spreadsheet.clone();
    let range = args.range.clone();
    let mode_owned = mode.to_string();
    let action = if mode == "append" {
        "Append"
    } else {
        "Overwrite"
    };
    let prompt = format!(
        "{action} {rows} row(s) at {range} in Spreadsheet {id}? This edits the live spreadsheet."
    );
    run_write_verb(
        verb,
        PLUGIN_SHEETS,
        args.workspace,
        args.json,
        args.yes,
        prompt,
        move |ws, dir| sheets_values_write_request(ws, dir, &mode_owned, &id, &range, &values),
        render_sheets_write,
    )
}

/// Human-readable write receipt for `values-update` / `values-append`.
fn render_sheets_write(value: &serde_json::Value) {
    println!(
        "bwoc gws {}: {} updated",
        field(value, "operation"),
        field(value, "updated_range"),
    );
    for (label, key) in [
        ("rows", "updated_rows"),
        ("columns", "updated_columns"),
        ("cells", "updated_cells"),
    ] {
        if let Some(n) = value.get(key).and_then(|v| v.as_i64()) {
            println!("  {label}: {n}");
        }
    }
}

const BAD_PRESENTATION_ID: &str =
    "presentation id must be 1..=512 chars of [A-Za-z0-9_-], no leading hyphen";

fn run_slides_get(args: SlidesGetArgs) -> i32 {
    let verb = "slides get";
    if !is_valid_resource_id(&args.presentation) {
        return usage_bad_field(verb, "bad_presentation_id", BAD_PRESENTATION_ID, args.json);
    }
    let id = args.presentation.clone();
    run_read_verb(
        verb,
        PLUGIN_SLIDES,
        args.workspace,
        args.json,
        move |ws, dir| slides_get_request(ws, dir, &id),
        |value| {
            let p = value.get("presentation").unwrap_or(value);
            println!("bwoc gws slides get: {}", field(p, "title"));
            println!("  presentation_id: {}", field(p, "presentation_id"));
            if let Some(n) = p.get("slide_count").and_then(|v| v.as_i64()) {
                println!("  slides: {n}");
            }
            if let Some(link) = p.get("web_view_link").and_then(|v| v.as_str()) {
                println!("  {link}");
            }
        },
    )
}

fn run_slides_batch_update(args: SlidesBatchUpdateArgs) -> i32 {
    let verb = "slides batch-update";
    if !is_valid_resource_id(&args.presentation) {
        return usage_bad_field(verb, "bad_presentation_id", BAD_PRESENTATION_ID, args.json);
    }
    let requests = match resolve_docs_requests(
        args.requests.as_deref(),
        args.requests_file.as_deref(),
        verb,
        args.json,
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let n = requests.as_array().map(|a| a.len()).unwrap_or(0);
    let id = args.presentation.clone();
    let prompt = format!(
        "Apply {n} batchUpdate request(s) to Presentation {id}? This edits the live presentation."
    );
    run_write_verb(
        verb,
        PLUGIN_SLIDES,
        args.workspace,
        args.json,
        args.yes,
        prompt,
        move |ws, dir| slides_batch_update_request(ws, dir, &id, &requests),
        render_slides_write,
    )
}

fn run_slides_replace_all_text(args: SlidesReplaceAllTextArgs) -> i32 {
    let verb = "slides replace-all-text";
    if !is_valid_resource_id(&args.presentation) {
        return usage_bad_field(verb, "bad_presentation_id", BAD_PRESENTATION_ID, args.json);
    }
    if !(1..=1024).contains(&args.find.len()) {
        return usage_bad_field(verb, "bad_find", "--find must be 1..=1024 bytes", args.json);
    }
    let id = args.presentation.clone();
    let find = args.find.clone();
    let replace = args.replace.clone();
    let match_case = args.match_case;
    let prompt = format!(
        "Replace all '{find}' → '{replace}' in Presentation {id}? This edits the live presentation."
    );
    run_write_verb(
        verb,
        PLUGIN_SLIDES,
        args.workspace,
        args.json,
        args.yes,
        prompt,
        move |ws, dir| slides_replace_all_text_request(ws, dir, &id, &find, &replace, match_case),
        render_slides_write,
    )
}

/// Human-readable write receipt for `slides batch-update` / `replace-all-text`.
fn render_slides_write(value: &serde_json::Value) {
    println!(
        "bwoc gws {}: applied to Presentation {}",
        field(value, "operation"),
        field(value, "presentation_id"),
    );
    if let Some(n) = value.get("requests_applied").and_then(|v| v.as_i64()) {
        println!("  requests applied: {n}");
    }
    if let Some(occ) = value.get("occurrences_changed").and_then(|v| v.as_i64()) {
        println!("  occurrences changed: {occ}");
    }
}

// ===========================================================================
// Tests — arg parsing, JSON shapes, id/query validation, pagination clamp,
// auth-shape probe, no-plugin stub path, never-leak guardrails.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::collections::HashMap;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        cmd: GwsCommand,
    }

    fn parse(args: &[&str]) -> Result<GwsCommand, clap::Error> {
        let mut full = vec!["bwoc-gws-test"];
        full.extend_from_slice(args);
        TestCli::try_parse_from(full).map(|c| c.cmd)
    }

    fn getenv_from(map: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| map.get(k).map(|v| v.to_string())
    }

    // --- arg parsing -------------------------------------------------------

    #[test]
    fn parses_auth_status() {
        match parse(&["auth", "status", "--json"]).unwrap() {
            GwsCommand::Auth(AuthCommand::Status(a)) => assert!(a.json),
            other => panic!("expected Auth::Status, got {other:?}"),
        }
    }

    #[test]
    fn parses_drive_list_with_query_and_max() {
        match parse(&[
            "drive",
            "list",
            "--query",
            "name contains 'spec'",
            "--max",
            "20",
            "--json",
        ])
        .unwrap()
        {
            GwsCommand::Drive(DriveCommand::List(a)) => {
                assert_eq!(a.query.as_deref(), Some("name contains 'spec'"));
                assert_eq!(a.max, Some(20));
                assert!(a.json);
            }
            other => panic!("expected Drive::List, got {other:?}"),
        }
    }

    #[test]
    fn parses_drive_show_requires_file() {
        match parse(&["drive", "show", "--file", "1AbC_dEf-123"]).unwrap() {
            GwsCommand::Drive(DriveCommand::Show(a)) => assert_eq!(a.file, "1AbC_dEf-123"),
            other => panic!("expected Drive::Show, got {other:?}"),
        }
        assert!(parse(&["drive", "show"]).is_err());
    }

    #[test]
    fn parses_gmail_verbs() {
        match parse(&["gmail", "search", "--query", "is:unread", "--max", "5"]).unwrap() {
            GwsCommand::Gmail(GmailCommand::Search(a)) => {
                assert_eq!(a.query.as_deref(), Some("is:unread"));
                assert_eq!(a.max, Some(5));
            }
            other => panic!("expected Gmail::Search, got {other:?}"),
        }
        match parse(&["gmail", "show", "--thread", "abc123"]).unwrap() {
            GwsCommand::Gmail(GmailCommand::Show(a)) => assert_eq!(a.thread, "abc123"),
            other => panic!("expected Gmail::Show, got {other:?}"),
        }
        match parse(&["gmail", "labels", "--json"]).unwrap() {
            GwsCommand::Gmail(GmailCommand::Labels(a)) => assert!(a.json),
            other => panic!("expected Gmail::Labels, got {other:?}"),
        }
        assert!(parse(&["gmail", "show"]).is_err());
    }

    #[test]
    fn parses_calendar_verbs() {
        match parse(&["calendar", "list"]).unwrap() {
            GwsCommand::Calendar(CalendarCommand::List(_)) => {}
            other => panic!("expected Calendar::List, got {other:?}"),
        }
        match parse(&["calendar", "events", "--calendar", "primary", "--max", "50"]).unwrap() {
            GwsCommand::Calendar(CalendarCommand::Events(a)) => {
                assert_eq!(a.calendar.as_deref(), Some("primary"));
                assert_eq!(a.max, Some(50));
            }
            other => panic!("expected Calendar::Events, got {other:?}"),
        }
    }

    #[test]
    fn parses_docs_get_requires_document() {
        match parse(&["docs", "get", "--document", "1AbC_dEf-123"]).unwrap() {
            GwsCommand::Docs(DocsCommand::Get(a)) => assert_eq!(a.document, "1AbC_dEf-123"),
            other => panic!("expected Docs::Get, got {other:?}"),
        }
        assert!(parse(&["docs", "get"]).is_err());
    }

    #[test]
    fn parses_docs_batch_update_needs_requests_source() {
        // Exactly one of --requests / --requests-file is required (ArgGroup).
        assert!(parse(&["docs", "batch-update", "--document", "abc"]).is_err());
        match parse(&[
            "docs",
            "batch-update",
            "--document",
            "abc",
            "--requests",
            "[{\"insertText\":{}}]",
            "--yes",
        ])
        .unwrap()
        {
            GwsCommand::Docs(DocsCommand::BatchUpdate(a)) => {
                assert_eq!(a.document, "abc");
                assert!(a.yes);
                assert!(a.requests.is_some());
                assert!(a.requests_file.is_none());
            }
            other => panic!("expected Docs::BatchUpdate, got {other:?}"),
        }
        // Both sources at once is rejected by the group.
        assert!(
            parse(&[
                "docs",
                "batch-update",
                "--document",
                "abc",
                "--requests",
                "[]",
                "--requests-file",
                "r.json",
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_docs_replace_all_text() {
        match parse(&[
            "docs",
            "replace-all-text",
            "--document",
            "abc",
            "--find",
            "March 31",
            "--replace",
            "In stock",
            "--match-case",
            "--yes",
        ])
        .unwrap()
        {
            GwsCommand::Docs(DocsCommand::ReplaceAllText(a)) => {
                assert_eq!(a.find, "March 31");
                assert_eq!(a.replace, "In stock");
                assert!(a.match_case);
                assert!(a.yes);
            }
            other => panic!("expected Docs::ReplaceAllText, got {other:?}"),
        }
        // --replace defaults to empty (a delete); --find is required.
        match parse(&[
            "docs",
            "replace-all-text",
            "--document",
            "abc",
            "--find",
            "x",
        ])
        .unwrap()
        {
            GwsCommand::Docs(DocsCommand::ReplaceAllText(a)) => assert_eq!(a.replace, ""),
            other => panic!("expected Docs::ReplaceAllText, got {other:?}"),
        }
        assert!(parse(&["docs", "replace-all-text", "--document", "abc"]).is_err());
    }

    #[test]
    fn docs_request_builders_carry_operation_and_fields() {
        let ws = Path::new("/ws");
        let dir = Path::new("/ws/modules/plugins/gws/gws-docs");
        let g = docs_get_request(ws, dir, "doc1");
        assert_eq!(g["operation"], "get");
        assert_eq!(g["document_id"], "doc1");

        let reqs =
            serde_json::json!([{ "insertText": { "location": { "index": 1 }, "text": "hi" } }]);
        let b = docs_batch_update_request(ws, dir, "doc1", &reqs);
        assert_eq!(b["operation"], "batch-update");
        assert_eq!(b["requests"], reqs);

        let r = docs_replace_all_text_request(ws, dir, "doc1", "a", "b", true);
        assert_eq!(r["operation"], "replace-all-text");
        assert_eq!(r["find"], "a");
        assert_eq!(r["replace"], "b");
        assert_eq!(r["match_case"], true);
    }

    #[test]
    fn resolve_docs_requests_validates_array() {
        // Valid non-empty array (inline).
        let v =
            resolve_docs_requests(Some("[{\"x\":1}]"), None, "docs batch-update", true).unwrap();
        assert!(v.as_array().is_some_and(|a| a.len() == 1));
        // Empty array is refused.
        assert!(resolve_docs_requests(Some("[]"), None, "docs batch-update", true).is_err());
        // Non-array is refused.
        assert!(resolve_docs_requests(Some("{}"), None, "docs batch-update", true).is_err());
        // Invalid JSON is refused.
        assert!(resolve_docs_requests(Some("nope"), None, "docs batch-update", true).is_err());
        // Neither source is refused.
        assert!(resolve_docs_requests(None, None, "docs batch-update", true).is_err());
    }

    #[test]
    fn docs_write_json_requires_yes() {
        // The write gate: --json without --yes cannot prompt → EXIT_USAGE.
        assert!(json_write_blocked(true, false));
        assert!(!json_write_blocked(true, true));
        assert!(!json_write_blocked(false, false));
    }

    #[test]
    fn parses_sheets_read_verbs() {
        match parse(&["sheets", "get", "--spreadsheet", "S1"]).unwrap() {
            GwsCommand::Sheets(SheetsCommand::Get(a)) => assert_eq!(a.spreadsheet, "S1"),
            other => panic!("expected Sheets::Get, got {other:?}"),
        }
        match parse(&[
            "sheets",
            "values-get",
            "--spreadsheet",
            "S1",
            "--range",
            "Sheet1!A1:B2",
        ])
        .unwrap()
        {
            GwsCommand::Sheets(SheetsCommand::ValuesGet(a)) => {
                assert_eq!(a.range, "Sheet1!A1:B2");
            }
            other => panic!("expected Sheets::ValuesGet, got {other:?}"),
        }
        assert!(parse(&["sheets", "values-get", "--spreadsheet", "S1"]).is_err());
    }

    #[test]
    fn parses_sheets_write_verbs_need_values_source() {
        // ArgGroup: exactly one of --values / --values-file.
        assert!(
            parse(&[
                "sheets",
                "values-update",
                "--spreadsheet",
                "S1",
                "--range",
                "A1"
            ])
            .is_err()
        );
        match parse(&[
            "sheets",
            "values-append",
            "--spreadsheet",
            "S1",
            "--range",
            "A1",
            "--values",
            "[[\"x\"]]",
            "--yes",
        ])
        .unwrap()
        {
            GwsCommand::Sheets(SheetsCommand::ValuesAppend(a)) => {
                assert!(a.yes);
                assert!(a.values.is_some());
            }
            other => panic!("expected Sheets::ValuesAppend, got {other:?}"),
        }
        assert!(
            parse(&[
                "sheets",
                "values-update",
                "--spreadsheet",
                "S1",
                "--range",
                "A1",
                "--values",
                "[[1]]",
                "--values-file",
                "v.json",
            ])
            .is_err()
        );
    }

    #[test]
    fn range_validation() {
        assert!(is_valid_range("Sheet1!A1:B2"));
        assert!(is_valid_range("'My Sheet'!A:A"));
        assert!(!is_valid_range("a/b")); // path segment
        assert!(!is_valid_range("")); // empty
        assert!(!is_valid_range(&"A".repeat(513))); // too long
    }

    #[test]
    fn resolve_sheet_values_validates_2d_array() {
        let v = resolve_sheet_values(Some("[[\"a\",\"b\"]]"), None, "sheets values-update", true)
            .unwrap();
        assert!(v.as_array().is_some_and(|r| r.len() == 1));
        // An empty outer array is refused (a no-op write).
        assert!(resolve_sheet_values(Some("[]"), None, "sheets values-update", true).is_err());
        // A flat array (not 2-D) is refused.
        assert!(resolve_sheet_values(Some("[\"a\"]"), None, "sheets values-update", true).is_err());
        // A non-array is refused.
        assert!(resolve_sheet_values(Some("{}"), None, "sheets values-update", true).is_err());
        // Invalid JSON is refused.
        assert!(resolve_sheet_values(Some("nope"), None, "sheets values-update", true).is_err());
        // Neither source is refused.
        assert!(resolve_sheet_values(None, None, "sheets values-update", true).is_err());
    }

    #[test]
    fn sheets_request_builders_carry_operation() {
        let ws = Path::new("/ws");
        let dir = Path::new("/ws/modules/plugins/gws/gws-sheets");
        assert_eq!(sheets_get_request(ws, dir, "S1")["operation"], "get");
        let vg = sheets_values_get_request(ws, dir, "S1", "A1:B2");
        assert_eq!(vg["operation"], "values-get");
        assert_eq!(vg["range"], "A1:B2");
        let vals = serde_json::json!([["a", "b"]]);
        let u = sheets_values_write_request(ws, dir, "update", "S1", "A1", &vals);
        assert_eq!(u["operation"], "values-update");
        assert_eq!(u["values"], vals);
        let ap = sheets_values_write_request(ws, dir, "append", "S1", "A1", &vals);
        assert_eq!(ap["operation"], "values-append");
    }

    #[test]
    fn parses_slides_verbs() {
        match parse(&["slides", "get", "--presentation", "P1"]).unwrap() {
            GwsCommand::Slides(SlidesCommand::Get(a)) => assert_eq!(a.presentation, "P1"),
            other => panic!("expected Slides::Get, got {other:?}"),
        }
        // batch-update needs a requests source (ArgGroup).
        assert!(parse(&["slides", "batch-update", "--presentation", "P1"]).is_err());
        match parse(&[
            "slides",
            "batch-update",
            "--presentation",
            "P1",
            "--requests",
            "[{\"createSlide\":{}}]",
            "--yes",
        ])
        .unwrap()
        {
            GwsCommand::Slides(SlidesCommand::BatchUpdate(a)) => {
                assert!(a.yes);
                assert!(a.requests.is_some());
            }
            other => panic!("expected Slides::BatchUpdate, got {other:?}"),
        }
        match parse(&[
            "slides",
            "replace-all-text",
            "--presentation",
            "P1",
            "--find",
            "{{title}}",
            "--replace",
            "Q3",
        ])
        .unwrap()
        {
            GwsCommand::Slides(SlidesCommand::ReplaceAllText(a)) => {
                assert_eq!(a.find, "{{title}}");
                assert_eq!(a.replace, "Q3");
            }
            other => panic!("expected Slides::ReplaceAllText, got {other:?}"),
        }
        assert!(parse(&["slides", "replace-all-text", "--presentation", "P1"]).is_err());
    }

    #[test]
    fn slides_request_builders_carry_operation() {
        let ws = Path::new("/ws");
        let dir = Path::new("/ws/modules/plugins/gws/gws-slides");
        assert_eq!(slides_get_request(ws, dir, "P1")["operation"], "get");
        let reqs = serde_json::json!([{ "createSlide": {} }]);
        let b = slides_batch_update_request(ws, dir, "P1", &reqs);
        assert_eq!(b["operation"], "batch-update");
        assert_eq!(b["requests"], reqs);
        let r = slides_replace_all_text_request(ws, dir, "P1", "a", "b", false);
        assert_eq!(r["operation"], "replace-all-text");
        assert_eq!(r["presentation_id"], "P1");
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(parse(&["frobnicate"]).is_err());
        assert!(parse(&["drive", "delete"]).is_err()); // read-mostly — no write verbs
    }

    // --- pagination clamp --------------------------------------------------

    #[test]
    fn normalize_max_clamps_to_window() {
        assert_eq!(normalize_max(None), None);
        assert_eq!(normalize_max(Some(0)), Some(1)); // zero is never intended
        assert_eq!(normalize_max(Some(20)), Some(20));
        assert_eq!(
            normalize_max(Some(MAX_RESULTS_CEILING)),
            Some(MAX_RESULTS_CEILING)
        );
        assert_eq!(normalize_max(Some(99_999)), Some(MAX_RESULTS_CEILING));
    }

    // --- id / query validation --------------------------------------------

    #[test]
    fn accepts_valid_resource_ids() {
        assert!(is_valid_resource_id("1AbC_dEfGhIjKlMnOpQrStUvWxYz"));
        assert!(is_valid_resource_id("abc-123_XYZ"));
        assert!(is_valid_resource_id("a"));
    }

    #[test]
    fn rejects_invalid_resource_ids() {
        assert!(!is_valid_resource_id("")); // empty
        assert!(!is_valid_resource_id("-leading-hyphen")); // option-injection guard
        assert!(!is_valid_resource_id("has spaces"));
        assert!(!is_valid_resource_id("has/slash"));
        assert!(!is_valid_resource_id("at@sign")); // '@' not allowed for file/thread ids
        assert!(!is_valid_resource_id(&"a".repeat(513))); // too long
    }

    #[test]
    fn calendar_id_allows_email_and_primary() {
        assert!(is_valid_calendar_id("primary"));
        assert!(is_valid_calendar_id("user@gmail.com"));
        assert!(is_valid_calendar_id("abc123@group.calendar.google.com"));
        assert!(!is_valid_calendar_id("-inject"));
        assert!(!is_valid_calendar_id("has space"));
        assert!(!is_valid_calendar_id(""));
    }

    #[test]
    fn query_validation() {
        assert!(is_valid_query("from:me is:unread"));
        assert!(is_valid_query("name contains 'spec'"));
        assert!(!is_valid_query("")); // empty
        assert!(!is_valid_query("has\nnewline")); // control byte
        assert!(!is_valid_query(&"q".repeat(1025))); // too long
    }

    // --- auth-shape probe (env + file, no network) -------------------------

    #[test]
    fn auth_shape_none_when_nothing_present() {
        let ws = tempfile::tempdir().unwrap();
        let env = getenv_from(HashMap::new());
        let shape = probe_auth_shape(ws.path(), &env);
        assert_eq!(shape.active_source, TokenSource::None);
        assert!(!shape.env_token_present);
        assert!(!shape.secrets_file_present);
        assert!(!shape.has_token());
    }

    #[test]
    fn auth_shape_env_wins_over_secrets_file() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join(".bwoc/secrets")).unwrap();
        std::fs::write(ws.path().join(SECRETS_REL), "{\"access_token\":\"secret\"}").unwrap();
        let env = getenv_from(HashMap::from([(ENV_TOKEN, "ya29.super-secret-token")]));
        let shape = probe_auth_shape(ws.path(), &env);
        assert_eq!(shape.active_source, TokenSource::Env);
        assert!(shape.env_token_present);
        assert!(shape.secrets_file_present);
        assert!(shape.has_token());
    }

    #[test]
    fn auth_shape_secrets_file_when_no_env() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join(".bwoc/secrets")).unwrap();
        std::fs::write(ws.path().join(SECRETS_REL), "{}").unwrap();
        let env = getenv_from(HashMap::new());
        let shape = probe_auth_shape(ws.path(), &env);
        assert_eq!(shape.active_source, TokenSource::SecretsFile);
        assert!(!shape.env_token_present);
        assert!(shape.secrets_file_present);
        assert!(shape.has_token());
    }

    #[test]
    fn auth_shape_serializes_source_as_kebab_case() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join(".bwoc/secrets")).unwrap();
        std::fs::write(ws.path().join(SECRETS_REL), "{}").unwrap();
        let env = getenv_from(HashMap::new());
        let shape = probe_auth_shape(ws.path(), &env);
        let json = serde_json::to_string(&shape).unwrap();
        assert!(
            json.contains("\"active_source\":\"secrets-file\""),
            "{json}"
        );
    }

    // --- never-leak guardrail ---------------------------------------------

    #[test]
    fn auth_shape_never_carries_token_value() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join(".bwoc/secrets")).unwrap();
        std::fs::write(
            ws.path().join(SECRETS_REL),
            "{\"access_token\":\"ya29.LEAK-ME-NOT\",\"refresh_token\":\"1//super-secret\"}",
        )
        .unwrap();
        let env = getenv_from(HashMap::from([(ENV_TOKEN, "ya29.ENV-LEAK-ME-NOT")]));
        let shape = probe_auth_shape(ws.path(), &env);
        let json = serde_json::to_string(&shape).unwrap();
        assert!(!json.contains("LEAK-ME-NOT"), "shape leaked token: {json}");
        assert!(
            !json.contains("super-secret"),
            "shape leaked refresh token: {json}"
        );
        assert!(!json.contains("ya29"), "shape leaked token prefix: {json}");
    }

    // --- request payload shapes (what the CLI hands the plugin) ------------

    #[test]
    fn auth_status_request_shape() {
        let v = auth_status_request(Path::new("/ws"), Path::new("/p"));
        assert_eq!(v["operation"], "status");
        assert_eq!(v["workspace"], "/ws");
        assert_eq!(v["plugin_dir"], "/p");
    }

    #[test]
    fn drive_list_request_carries_query_and_max() {
        let v = drive_list_request(Path::new("/ws"), Path::new("/p"), Some("q"), Some(10));
        assert_eq!(v["operation"], "list");
        assert_eq!(v["query"], "q");
        assert_eq!(v["max"], 10);
        // Omitted optionals serialize as null, not missing.
        let bare = drive_list_request(Path::new("/ws"), Path::new("/p"), None, None);
        assert!(bare["query"].is_null());
        assert!(bare["max"].is_null());
    }

    #[test]
    fn drive_show_request_shape() {
        let v = drive_show_request(Path::new("/ws"), Path::new("/p"), "file-1");
        assert_eq!(v["operation"], "get");
        assert_eq!(v["file_id"], "file-1");
    }

    #[test]
    fn gmail_request_shapes() {
        let s = gmail_search_request(
            Path::new("/ws"),
            Path::new("/p"),
            Some("is:unread"),
            Some(3),
        );
        assert_eq!(s["operation"], "search");
        assert_eq!(s["query"], "is:unread");
        assert_eq!(s["max"], 3);
        let show = gmail_show_request(Path::new("/ws"), Path::new("/p"), "t-1");
        assert_eq!(show["operation"], "show");
        assert_eq!(show["thread_id"], "t-1");
        let labels = gmail_labels_request(Path::new("/ws"), Path::new("/p"));
        assert_eq!(labels["operation"], "labels");
    }

    #[test]
    fn calendar_request_shapes() {
        let list = calendar_list_request(Path::new("/ws"), Path::new("/p"));
        assert_eq!(list["operation"], "calendars");
        let events =
            calendar_events_request(Path::new("/ws"), Path::new("/p"), Some("primary"), Some(7));
        assert_eq!(events["operation"], "events");
        assert_eq!(events["calendar_id"], "primary");
        assert_eq!(events["max"], 7);
        let bare = calendar_events_request(Path::new("/ws"), Path::new("/p"), None, None);
        assert!(bare["calendar_id"].is_null());
        assert!(bare["max"].is_null());
    }

    // --- plugin discovery / stub-error path --------------------------------

    fn write_workspace(root: &Path, workspace_toml: &str) {
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(root.join(".bwoc/workspace.toml"), workspace_toml).unwrap();
    }

    fn write_plugin_at(root: &Path, layout: &str, name: &str, kind: &str) {
        let dir = root.join("modules/plugins").join(layout).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "[plugin]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
                 description = \"x\"\ncompat = \">=2.5.0\"\nentry = \"gws.sh\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn no_plugins_dir_discovers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_plugin(dir.path(), PLUGIN_DRIVE).unwrap().is_none());
    }

    #[test]
    fn discovers_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", PLUGIN_DRIVE, "gws");
        let p = discover_plugin(dir.path(), PLUGIN_DRIVE).unwrap().unwrap();
        assert_eq!(p.name, PLUGIN_DRIVE);
        assert_eq!(p.entry, "gws.sh");
    }

    #[test]
    fn discovers_gws_namespaced_layout() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "gws", PLUGIN_GMAIL, "gws");
        let p = discover_plugin(dir.path(), PLUGIN_GMAIL).unwrap().unwrap();
        assert_eq!(p.name, PLUGIN_GMAIL);
    }

    #[test]
    fn discovery_rejects_wrong_kind() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", PLUGIN_AUTH, "workflow");
        let err = discover_plugin(dir.path(), PLUGIN_AUTH).unwrap_err();
        assert!(err.contains("expected"), "{err}");
        assert!(err.contains("gws"), "{err}");
    }

    #[test]
    fn enabled_plugin_requires_enabled_flag() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", PLUGIN_CALENDAR, "gws");
        // installed but disabled → stub path.
        write_workspace(dir.path(), "[plugins.gws-calendar]\nenabled = false\n");
        assert!(
            find_enabled_plugin(dir.path(), PLUGIN_CALENDAR)
                .unwrap()
                .is_none()
        );
        // enabled → discovered.
        write_workspace(dir.path(), "[plugins.gws-calendar]\nenabled = true\n");
        let p = find_enabled_plugin(dir.path(), PLUGIN_CALENDAR)
            .unwrap()
            .unwrap();
        assert_eq!(p.name, PLUGIN_CALENDAR);
    }

    #[test]
    fn no_plugin_message_names_install_command() {
        let m = no_plugin_message(PLUGIN_DRIVE);
        assert!(m.contains(PLUGIN_DRIVE));
        assert!(m.contains("bwoc plugin install"));
        assert!(m.contains("bwoc plugin enable"));
    }

    // --- token gate --------------------------------------------------------

    #[test]
    fn require_token_passes_when_present_blocks_when_absent() {
        let present = AuthShape {
            active_source: TokenSource::Env,
            env_token_present: true,
            secrets_file_present: false,
        };
        assert!(require_token(&present, "drive list", true).is_ok());
        let absent = AuthShape {
            active_source: TokenSource::None,
            env_token_present: false,
            secrets_file_present: false,
        };
        assert_eq!(require_token(&absent, "drive list", true), Err(EXIT_USAGE));
    }
}
