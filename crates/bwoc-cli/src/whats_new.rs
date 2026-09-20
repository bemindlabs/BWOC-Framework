//! "What's New" surface — shown two ways:
//!   1. A full section in the no-subcommand banner (always visible there).
//!   2. A one-line upgrade notice on any subcommand, printed once per
//!      MAJOR.MINOR change (npm-style) so it never spams across patch
//!      bumps and never pollutes piped/`--json` stdout.
//!
//! Highlights live here as the single source — the banner imports them.
//! The `HEADLINE` version is derived from Cargo at compile time; update only
//! its prose tagline + `HIGHLIGHTS` on each release that's worth shouting.

use std::io::IsTerminal;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One-line headline for the current release. The `MAJOR.MINOR` is built from
/// Cargo's version at compile time (`concat!` + `env!`) so the auto-version
/// hook can never desync the headline from the binary it ships in (BWOC-32).
pub const HEADLINE: &str = concat!(
    "BWOC ",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    " — a session that can answer for itself: thirteen `/` commands for what it is, what it may do and what it costs, plus a fixed context pane"
);

/// Short highlight bullets for the current MAJOR.MINOR. Keep ≤6, each a
/// single line — they render in the banner and inform the upgrade notice.
///
/// RELEASE GATE: at least one bullet must cite the current `MAJOR.MINOR`
/// (see `highlights_cite_current_version` below). This fails CI the moment
/// the auto-version hook bumps the minor without anyone refreshing this
/// prose — i.e. "update What's New every release" is enforced, not trusted.
pub const HIGHLIGHTS: &[&str] = &[
    "**A session that can answer for itself.** `/status`, `/tools`, `/cost`, `/models`, `/backends`, `/settings` and `/doctor` report what the session is and what it runs on; `/permissions`, `/mcp` and `/context` report what it may do, what it is connected to and what fills the model's window; `/compact` folds context on demand, `/retry` resends and `/save` writes the transcript. A fixed right pane shows the directory and branch, the workspace and its agents, and the files this session changed (3.6.0, #545–#547)",
    "**A chat session you can steer.** `Esc` cancels the turn in flight; `/model <name>` switches models mid-session; `write_file` / `edit_file` / `multi_edit` show a unified diff of what changed on disk; `/sessions`, `/session <id>`, `/new` and `/fork` move between this directory's conversations without leaving the TUI; answers render as Markdown; `/undo` and `/redo` take a turn's file changes back and put them again (3.5.0, #540–#542)",
    "**`/` commands and `@` files in the chat input.** `/` opens a command menu (`/help`, `/clear`, `/mode`, `/quit`) that runs in the TUI, never reaching the model; `@` completes project files and attaches each text file inside the working directory to your message (32 KB cap). `bwoc dashboard` is deprecated — use `bwoc fleet` (3.4.0, #535, #537)",
    "**Several conversations per directory.** Bare `bwoc` still resumes the latest; `bwoc --new` starts another, `bwoc --session <id>` (id or prefix) resumes one, and `bwoc session list`, `bwoc session fork` and `bwoc session rm` manage them. `bwoc --backend claude` (or `codex`, `agy`, `kimi`, `grok`, `copilot`) opens that CLI on its own login (3.3.0, #530–#532)",
    "**`bwoc` is a coding agent.** On a terminal, bare `bwoc` opens a coding session in the current directory — provider from flags, env, `.bwoc/config.toml` or auto-detect (Anthropic key or local Ollama), keys via `bwoc auth`. Chat runs the batch retry, MCP and sandbox paths, sizes compaction to the model, and adds `glob`, regex `grep`, `multi_edit`, `webfetch`, `todo`, `subagent` and a `run_command` timeout (3.2.0, #519–#526)",
    "**bwoc-bot — one agent, one bot.** A connector `[bot]` block answers slash commands without the model and caps each sender's rate and message length; `public = true` opts into a limited public mode — strangers reach the agent only by DM or @mention, in read-only `plan` mode, inside an isolated workdir with no memories or other chats (3.1.0, #506)",
];

/// `MAJOR.MINOR` of the current build (the patch component churns on every
/// edit via the auto-version hook, so the upgrade notice keys on the
/// release-significant prefix only).
fn major_minor() -> String {
    let mut it = VERSION.split('.');
    let major = it.next().unwrap_or("0");
    let minor = it.next().unwrap_or("0");
    format!("{major}.{minor}")
}

/// Print a one-line "you upgraded" notice to **stderr** if the stored
/// last-seen MAJOR.MINOR differs from this build, then record the current
/// one. No-op when:
///   - stdout is not a TTY (pipes / CI / `--json` consumers)
///   - `BWOC_NO_WHATSNEW=1` is set
///   - `~/.bwoc/` is unavailable (best-effort — never blocks a command)
///
/// Call this for subcommands only; the bare-`bwoc` banner already shows
/// the full What's New block.
pub fn notify_if_updated() {
    if std::env::var_os("BWOC_NO_WHATSNEW").is_some() {
        return;
    }
    // Gate on stdout TTY so piped/scripted output stays clean even though
    // we print to stderr (a consumer tailing both shouldn't get surprised).
    if !std::io::stdout().is_terminal() {
        return;
    }
    let Ok(home) = crate::user_home::bwoc_home() else {
        return;
    };
    let marker = home.join("last-seen-version");
    let current = major_minor();
    let seen = std::fs::read_to_string(&marker)
        .ok()
        .map(|s| s.trim().to_string());
    if seen.as_deref() == Some(current.as_str()) {
        return; // already greeted on this MAJOR.MINOR
    }
    // Record first so a write failure doesn't loop the notice forever.
    let _ = std::fs::write(&marker, &current);

    let tty = std::io::stderr().is_terminal();
    let (cyan, dim, reset) = if tty {
        ("\x1b[1;36m", "\x1b[2m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    eprintln!(
        "{cyan}✨ {HEADLINE}{reset}  {dim}(run `bwoc` for what's new · `BWOC_NO_WHATSNEW=1` to hush){reset}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_minor_strips_patch() {
        // VERSION is the live Cargo version (e.g. 2.0.48) — assert the
        // prefix shape, not a literal that the auto-version hook churns.
        let mm = major_minor();
        let parts: Vec<&str> = mm.split('.').collect();
        assert_eq!(parts.len(), 2, "major.minor has exactly two parts");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }

    #[test]
    fn highlights_are_lean() {
        assert!(!HIGHLIGHTS.is_empty());
        assert!(HIGHLIGHTS.len() <= 6, "keep the What's New list short");
        assert!(HIGHLIGHTS.iter().all(|h| !h.contains('\n')));
    }

    #[test]
    fn highlights_cite_current_version() {
        // RELEASE GATE — the stale-prose guard. The HEADLINE *number* is
        // compile-derived so it can never lag the build, but the tagline and
        // these bullets are hand-written and silently rotted for many releases
        // (e.g. stuck on a `gcloud IAM` headline while the binary was several
        // minors ahead). Require at least one bullet to name the current
        // `MAJOR.MINOR`: the auto-version hook bumps the minor on release, this
        // assertion then fails until someone refreshes the prose, so "update
        // What's New every release" is enforced by CI rather than remembered.
        let mm = major_minor();
        let cites = HIGHLIGHTS.iter().any(|h| h.contains(&mm));
        assert!(
            cites,
            "no HIGHLIGHTS bullet cites the current version {mm} — refresh the \
             HIGHLIGHTS list for this release (cite `{mm}.x` in a bullet). The \
             HEADLINE tagline usually wants a pass too, but only HIGHLIGHTS is \
             enforced here."
        );
    }

    #[test]
    fn headline_version_matches_build() {
        // Guard against the stale-HEADLINE class of bug: the headline must
        // name the current MAJOR.MINOR, so a `bwoc` build never greets users
        // with a version it isn't. Bumping Cargo without updating HEADLINE
        // fails here (same lesson as the formula auto-bump, #52).
        let expected = format!("BWOC {}", major_minor());
        assert!(
            HEADLINE.starts_with(&expected),
            "HEADLINE {HEADLINE:?} must start with {expected:?} (CARGO_PKG_VERSION major.minor)"
        );
    }
}
