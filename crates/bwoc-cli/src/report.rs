//! `bwoc report` — file a GitHub issue against the BWOC framework from the CLI.
//!
//! Two paths, chosen fail-safe:
//!
//! - **Create** (`gh issue create`): only on an interactive terminal after an
//!   explicit confirmation (or `--yes`). Creating a public issue is an
//!   outward-facing act — it is never done unattended.
//! - **URL** (prefilled `issues/new?...`): when `--web` is passed, when `gh`
//!   is unavailable/unauthenticated, or when the session is non-interactive
//!   without `--yes`. The user opens the link and submits themselves.
//!
//! The body always carries an **Environment** block (bwoc version, release
//! identity, OS/arch) so a report is diagnosable without a follow-up round.
//! Shells out via the same [`ShellRunner`] seam as `bwoc update` — no new
//! HTTP dependency, and unit tests inject a mock.

use crate::update::{GITHUB_REPO, ProcessShellRunner, ShellRunner};

pub struct ReportArgs {
    /// Issue title (required — there is no useful default).
    pub title: Option<String>,
    /// Issue body; the Environment block is appended either way.
    pub body: Option<String>,
    /// Report kind, mapped to a GitHub label.
    pub kind: String,
    /// Print the prefilled browser URL instead of creating via `gh`.
    pub web: bool,
    /// Skip the interactive confirmation (still requires a resolvable `gh`).
    pub yes: bool,
}

pub fn run(args: ReportArgs) -> i32 {
    use std::io::IsTerminal;
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    run_inner(args, &ProcessShellRunner, tty)
}

/// Testable core: TTY-ness injected, shell-outs via the runner seam.
fn run_inner(args: ReportArgs, runner: &dyn ShellRunner, tty: bool) -> i32 {
    let Some(title) = args
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        eprintln!("bwoc report: a title is required — bwoc report \"<title>\" [--body \"…\"]");
        return 2;
    };
    let Some(label) = kind_label(&args.kind) else {
        eprintln!(
            "bwoc report: unknown --kind '{}' (use bug | feature | question)",
            args.kind
        );
        return 2;
    };
    let body = build_body(args.body.as_deref());
    let url = issue_url(GITHUB_REPO, title, &body, label);

    // `gh auth status` exits 0 only when a usable login exists.
    let gh_ok = runner.run("gh", &["auth", "status"]).exit_code == 0;

    // Fail-safe to the browser URL: requested, no usable gh, or unattended
    // without an explicit --yes (a public issue is never filed unattended).
    if args.web || !gh_ok || (!tty && !args.yes) {
        println!("{url}");
        if !gh_ok && !args.web {
            eprintln!("(gh is not available/authenticated — open the URL above to submit)");
        }
        return 0;
    }

    // Preview, then confirm (skipped by --yes).
    println!("Will create a GitHub issue in {GITHUB_REPO}:\n");
    println!("  title : {title}");
    println!("  label : {label}\n");
    println!("{body}\n");
    if !args.yes {
        eprint!("Create this issue? [y/N] ");
        let mut line = String::new();
        let confirmed = std::io::stdin().read_line(&mut line).is_ok()
            && matches!(line.trim(), "y" | "Y" | "yes");
        if !confirmed {
            eprintln!("aborted — nothing filed. (Tip: --web prints a prefilled browser URL.)");
            return 1;
        }
    }

    let out = runner.run(
        "gh",
        &[
            "issue",
            "create",
            "--repo",
            GITHUB_REPO,
            "--title",
            title,
            "--body",
            &body,
            "--label",
            label,
        ],
    );
    if out.exit_code == 0 {
        println!("{}", out.stdout);
        0
    } else {
        eprintln!(
            "bwoc report: `gh issue create` failed (exit {}). Submit via the browser instead:\n{url}",
            out.exit_code
        );
        1
    }
}

/// Map the report kind to a GitHub label that exists by default on the repo
/// (`feature` → the stock `enhancement` label; there is no stock `feature`).
fn kind_label(kind: &str) -> Option<&'static str> {
    match kind {
        "bug" => Some("bug"),
        "feature" => Some("enhancement"),
        "question" => Some("question"),
        _ => None,
    }
}

/// User body (or a placeholder) + the Environment block.
fn build_body(user: Option<&str>) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let release = option_env!("BWOC_RELEASE_CALVER").unwrap_or("source build");
    let lead = match user.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => b.to_string(),
        None => "(describe the issue here)".to_string(),
    };
    format!(
        "{lead}\n\n---\n**Environment**\n- bwoc: {version} ({release})\n- os: {}/{}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Prefilled new-issue URL (`title`/`body`/`labels` query params).
fn issue_url(repo: &str, title: &str, body: &str, label: &str) -> String {
    format!(
        "https://github.com/{repo}/issues/new?title={}&body={}&labels={}",
        percent_encode(title),
        percent_encode(body),
        percent_encode(label)
    )
}

/// RFC 3986 percent-encoding: unreserved characters pass through, everything
/// else (UTF-8 bytes included) becomes `%XX`. Tiny and dependency-free.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::ShellOutcome;
    use std::sync::Mutex;

    /// Records every shell-out; scripts `gh auth status` and `gh issue create`.
    struct MockRunner {
        gh_authed: bool,
        create_exit: i32,
        calls: Mutex<Vec<String>>,
    }
    impl ShellRunner for MockRunner {
        fn run(&self, program: &str, args: &[&str]) -> ShellOutcome {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{program} {}", args.join(" ")));
            match args.first().copied() {
                Some("auth") => ShellOutcome {
                    exit_code: if self.gh_authed { 0 } else { 1 },
                    stdout: String::new(),
                },
                Some("issue") => ShellOutcome {
                    exit_code: self.create_exit,
                    stdout: "https://github.com/bemindlabs/BWOC-Framework/issues/999".into(),
                },
                _ => ShellOutcome {
                    exit_code: 127,
                    stdout: String::new(),
                },
            }
        }
    }

    fn mock(authed: bool) -> MockRunner {
        MockRunner {
            gh_authed: authed,
            create_exit: 0,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn args(title: &str) -> ReportArgs {
        ReportArgs {
            title: Some(title.to_string()),
            body: None,
            kind: "bug".to_string(),
            web: false,
            yes: true,
        }
    }

    #[test]
    fn percent_encode_handles_space_and_utf8() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(
            percent_encode("ครับ"),
            "%E0%B8%84%E0%B8%A3%E0%B8%B1%E0%B8%9A"
        );
        assert_eq!(percent_encode("A-z_.~0"), "A-z_.~0");
    }

    #[test]
    fn body_carries_environment_block() {
        let b = build_body(Some("it broke"));
        assert!(b.starts_with("it broke"));
        assert!(b.contains("**Environment**"));
        assert!(b.contains(env!("CARGO_PKG_VERSION")));
        assert!(build_body(None).contains("(describe the issue here)"));
    }

    #[test]
    fn kind_maps_feature_to_enhancement_and_rejects_unknown() {
        assert_eq!(kind_label("feature"), Some("enhancement"));
        assert_eq!(kind_label("bug"), Some("bug"));
        assert_eq!(kind_label("urgent"), None);
    }

    #[test]
    fn missing_title_or_bad_kind_is_usage_error() {
        let r = mock(true);
        let mut a = args(" ");
        assert_eq!(run_inner(a, &r, true), 2);
        a = args("t");
        a.kind = "urgent".into();
        assert_eq!(run_inner(a, &r, true), 2);
        assert!(
            r.calls.lock().unwrap().is_empty(),
            "no shell-outs on usage errors"
        );
    }

    #[test]
    fn web_flag_prints_url_and_never_creates() {
        let r = mock(true);
        let mut a = args("title");
        a.web = true;
        assert_eq!(run_inner(a, &r, true), 0);
        assert!(
            !r.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.contains("issue create")),
            "must not create when --web"
        );
    }

    #[test]
    fn non_tty_without_yes_falls_back_to_url() {
        let r = mock(true);
        let mut a = args("title");
        a.yes = false;
        assert_eq!(run_inner(a, &r, false), 0);
        assert!(
            !r.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.contains("issue create")),
            "unattended sessions never file issues"
        );
    }

    #[test]
    fn unauthenticated_gh_falls_back_to_url() {
        let r = mock(false);
        assert_eq!(run_inner(args("title"), &r, true), 0);
        assert!(
            !r.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.contains("issue create"))
        );
    }

    #[test]
    fn yes_on_tty_creates_with_repo_title_body_label() {
        let r = mock(true);
        let mut a = args("crash on spawn");
        a.body = Some("steps: …".into());
        a.kind = "feature".into();
        assert_eq!(run_inner(a, &r, true), 0);
        let calls = r.calls.lock().unwrap();
        let create = calls
            .iter()
            .find(|c| c.contains("issue create"))
            .expect("created");
        assert!(create.contains("--repo bemindlabs/BWOC-Framework"));
        assert!(create.contains("--title crash on spawn"));
        assert!(create.contains("--label enhancement"));
        assert!(create.contains("**Environment**"));
    }

    #[test]
    fn failed_create_reports_url_fallback_and_exit_1() {
        let r = MockRunner {
            gh_authed: true,
            create_exit: 1,
            calls: Mutex::new(Vec::new()),
        };
        assert_eq!(run_inner(args("t"), &r, true), 1);
    }
}
