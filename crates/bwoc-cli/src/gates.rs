//! `bwoc gates` — evaluate the four Padhana verification gates (Sammā-vayama,
//! AGENTS.md §6) from an agent's `config.manifest.json`.
//!
//! Mapping (see `docs/en/INCARNATION.en.md` (section "Verification Gates — `bwoc gates`")):
//!
//! | Gate | Meaning | Manifest key |
//! |---|---|---|
//! | Samvara | guard — static analysis | `lintCmd` |
//! | Pahana | abandon ill — format check | `formatCmd` |
//! | Bhavana | cultivate good — tests | `testCmd` |
//! | Anurakkhana | sustain — build before push | `buildCmd` |
//!
//! SECURITY: the commands are UNTRUSTED manifest input. Default is a dry run
//! that only prints them; `--run` executes each via `sh -c` with cwd = the agent
//! dir, a scrubbed (allowlist-only) environment, and a bounded timeout.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bwoc_core::manifest::Manifest;

use crate::i18n;

/// Per-gate wall-clock budget when running with `--run`.
const GATE_TIMEOUT: Duration = Duration::from_secs(600);

pub struct GatesArgs {
    pub path: Option<PathBuf>,
    pub run: bool,
    pub json: bool,
    pub lang: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Samvara,
    Pahana,
    Bhavana,
    Anurakkhana,
}

const ALL_GATES: [Gate; 4] = [
    Gate::Samvara,
    Gate::Pahana,
    Gate::Bhavana,
    Gate::Anurakkhana,
];

impl Gate {
    fn pali(self) -> &'static str {
        match self {
            Gate::Samvara => "Samvara",
            Gate::Pahana => "Pahana",
            Gate::Bhavana => "Bhavana",
            Gate::Anurakkhana => "Anurakkhana",
        }
    }
    fn manifest_key(self) -> &'static str {
        match self {
            Gate::Samvara => "lintCmd",
            Gate::Pahana => "formatCmd",
            Gate::Bhavana => "testCmd",
            Gate::Anurakkhana => "buildCmd",
        }
    }
    fn i18n_key(self) -> &'static str {
        match self {
            Gate::Samvara => "gates-samvara",
            Gate::Pahana => "gates-pahana",
            Gate::Bhavana => "gates-bhavana",
            Gate::Anurakkhana => "gates-anurakkhana",
        }
    }
    fn command(self, m: &Manifest) -> &str {
        match self {
            Gate::Samvara => &m.lint_cmd,
            Gate::Pahana => &m.format_cmd,
            Gate::Bhavana => &m.test_cmd,
            Gate::Anurakkhana => &m.build_cmd,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Dry run: command configured, not executed.
    Pending,
    Pass,
    Fail,
    Unconfigured,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "PENDING",
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Unconfigured => "UNCONFIGURED",
        }
    }
    fn is_bad(self) -> bool {
        matches!(self, Status::Fail | Status::Unconfigured)
    }
}

/// True for empty commands, unsubstituted `{{placeholder}}`s and the template's
/// `echo 'gate: ...'` stubs — none of which verify anything.
fn is_unconfigured(cmd: &str) -> bool {
    let c = cmd.trim();
    if c.is_empty() || c.contains("{{") || c.starts_with('#') {
        return true;
    }
    let lower = c.to_ascii_lowercase();
    ["echo 'gate:", "echo \"gate:", "echo gate:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

struct GateResult {
    gate: Gate,
    command: String,
    status: Status,
    exit_code: Option<i32>,
    duration_ms: u64,
}

/// Run one command via `sh -c` — cwd = agent dir, scrubbed env, timeout. The
/// command string is passed as a single argv element; nothing else is interpolated.
fn execute(cmd: &str, dir: &Path, timeout: Duration, quiet: bool) -> (Status, Option<i32>) {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .env_clear()
        .envs(bwoc_core::env_scrub::scrub_env())
        .stdin(Stdio::null())
        // --json: keep stdout a single JSON document, so drop the gate's own output.
        .stdout(if quiet {
            Stdio::null()
        } else {
            Stdio::inherit()
        });
    // Own process group, so a timeout can kill the gate's grandchildren too.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bwoc gates: spawn failed: {e}");
            return (Status::Fail, None);
        }
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let code = st.code();
                return (
                    if st.success() {
                        Status::Pass
                    } else {
                        Status::Fail
                    },
                    code,
                );
            }
            Ok(None) if start.elapsed() >= timeout => {
                kill_gate(&mut child);
                let _ = child.wait();
                eprintln!("bwoc gates: timed out after {}s", timeout.as_secs());
                return (Status::Fail, None);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return (Status::Fail, None),
        }
    }
}

/// Kill the gate and everything it spawned. Unix: SIGKILL the whole process
/// group (the child is its leader via `process_group(0)`); elsewhere: kill the
/// direct child only.
#[cfg(unix)]
fn kill_gate(child: &mut std::process::Child) {
    // SAFETY: plain kill(2) on the group we created for this child.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_gate(child: &mut std::process::Child) {
    let _ = child.kill();
}

fn evaluate(
    m: &Manifest,
    dir: &Path,
    run: bool,
    timeout: Duration,
    quiet: bool,
) -> Vec<GateResult> {
    ALL_GATES
        .iter()
        .map(|&gate| {
            let command = gate.command(m).to_string();
            let started = Instant::now();
            let (status, exit_code) = if is_unconfigured(&command) {
                (Status::Unconfigured, None)
            } else if !run {
                (Status::Pending, None)
            } else {
                execute(&command, dir, timeout, quiet)
            };
            GateResult {
                gate,
                command,
                status,
                exit_code,
                duration_ms: started.elapsed().as_millis() as u64,
            }
        })
        .collect()
}

fn results_json(agent: &str, run: bool, results: &[GateResult]) -> serde_json::Value {
    serde_json::json!({
        "agent": agent,
        "executed": run,
        "ok": !results.iter().any(|r| r.status.is_bad()),
        "gates": results.iter().map(|r| serde_json::json!({
            "gate": r.gate.pali(),
            "key": r.gate.manifest_key(),
            "command": r.command,
            "status": r.status.as_str(),
            "exit_code": r.exit_code,
            "duration_ms": r.duration_ms,
        })).collect::<Vec<_>>(),
    })
}

pub fn run(args: GatesArgs) -> i32 {
    let dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let manifest_path = dir.join("config.manifest.json");
    let manifest = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc gates: cannot load {}: {e}", manifest_path.display());
            return crate::exit::ERROR;
        }
    };
    let results = evaluate(&manifest, &dir, args.run, GATE_TIMEOUT, args.json);
    let bad = results.iter().filter(|r| r.status.is_bad()).count();

    if args.json {
        match serde_json::to_string_pretty(&results_json(&manifest.agent_id, args.run, &results)) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("bwoc gates: failed to serialize JSON: {e}");
                return crate::exit::ERROR;
            }
        }
    } else {
        let b = i18n::bundle_for(&args.lang);
        println!(
            "{}",
            i18n::t_with(&b, "gates-title", &[("agent", &manifest.agent_id)])
        );
        for r in &results {
            println!(
                "{:<13} {}\n    {} = {}",
                r.status.as_str(),
                i18n::t(&b, r.gate.i18n_key()),
                r.gate.manifest_key(),
                r.command
            );
        }
        println!();
        if !args.run {
            println!("{}", i18n::t(&b, "gates-dry-note"));
        }
        let summary = if bad > 0 {
            i18n::t_with(&b, "gates-summary-fail", &[("count", &bad.to_string())])
        } else if args.run {
            i18n::t(&b, "gates-summary-ok")
        } else {
            i18n::t(&b, "gates-summary-dry")
        };
        println!("{summary}");
    }
    if bad > 0 {
        crate::exit::FINDINGS
    } else {
        crate::exit::OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(lint: &str, fmt: &str, test: &str, build: &str) -> Manifest {
        let json = serde_json::json!({
            "name": "t", "version": "3.0", "agentId": "agent-t", "agentRole": "r",
            "primaryModel": "m", "memoryPath": "memories/",
            "lintCmd": lint, "formatCmd": fmt, "testCmd": test, "buildCmd": build,
        });
        serde_json::from_value(json).expect("manifest parses")
    }

    #[test]
    fn mapping_is_lint_format_test_build() {
        let m = manifest("L", "F", "T", "B");
        let got: Vec<_> = ALL_GATES.iter().map(|g| g.command(&m)).collect();
        assert_eq!(got, ["L", "F", "T", "B"]);
        assert_eq!(Gate::Samvara.manifest_key(), "lintCmd");
        assert_eq!(Gate::Anurakkhana.manifest_key(), "buildCmd");
    }

    #[test]
    fn stubs_and_placeholders_are_unconfigured() {
        for c in [
            "",
            "   ",
            "{{lintCmd}}",
            "echo 'gate: static analysis'",
            "echo \"gate: x\"",
            "# (none)",
        ] {
            assert!(is_unconfigured(c), "{c:?}");
        }
        assert!(!is_unconfigured("cargo clippy"));
        assert!(!is_unconfigured("echo hello"));
    }

    #[test]
    fn dry_run_never_executes() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let cmd = format!("touch {}", marker.display());
        let m = manifest(&cmd, &cmd, &cmd, &cmd);
        let r = evaluate(&m, dir.path(), false, Duration::from_secs(5), false);
        assert!(r.iter().all(|x| x.status == Status::Pending));
        assert!(!marker.exists(), "dry run must not execute commands");
    }

    #[test]
    fn run_reports_pass_fail_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        let m = manifest("true", "false", "{{testCmd}}", "exit 7");
        let r = evaluate(&m, dir.path(), true, Duration::from_secs(5), false);
        assert_eq!(r[0].status, Status::Pass);
        assert_eq!(r[1].status, Status::Fail);
        assert_eq!(r[2].status, Status::Unconfigured);
        assert_eq!(r[3].status, Status::Fail);
        assert_eq!(r[3].exit_code, Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_backgrounded_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        let cmd = format!("sleep 30 & echo $! > {}; wait", pidfile.display());
        let (s, _) = execute(&cmd, dir.path(), Duration::from_millis(500), true);
        assert_eq!(s, Status::Fail);
        let pid: libc::pid_t = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut gone = false;
        for _ in 0..40 {
            // kill(pid, 0) fails with ESRCH once the process is gone.
            if unsafe { libc::kill(pid, 0) } != 0 {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(gone, "grandchild survived the timeout");
    }

    #[test]
    fn timeout_kills_and_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (s, _) = execute("sleep 5", dir.path(), Duration::from_millis(200), false);
        assert_eq!(s, Status::Fail);
    }

    #[test]
    fn cwd_is_agent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (s, _) = execute(
            "touch cwd-marker",
            dir.path(),
            Duration::from_secs(5),
            false,
        );
        assert_eq!(s, Status::Pass);
        assert!(dir.path().join("cwd-marker").exists());
    }

    #[test]
    fn json_shape() {
        let dir = tempfile::tempdir().unwrap();
        let m = manifest("true", "{{x}}", "true", "true");
        let r = evaluate(&m, dir.path(), true, Duration::from_secs(5), false);
        let j = results_json("agent-t", true, &r);
        assert_eq!(j["agent"], "agent-t");
        assert_eq!(j["executed"], true);
        assert_eq!(j["ok"], false);
        assert_eq!(j["gates"].as_array().unwrap().len(), 4);
        assert_eq!(j["gates"][0]["gate"], "Samvara");
        assert_eq!(j["gates"][0]["status"], "PASS");
        assert_eq!(j["gates"][1]["status"], "UNCONFIGURED");
        assert!(j["gates"][1]["exit_code"].is_null());
    }

    #[test]
    fn failing_gate_exits_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let m = manifest("true", "true", "false", "true");
        let json = serde_json::to_string(&m).unwrap();
        std::fs::write(dir.path().join("config.manifest.json"), json).unwrap();
        let code = |run| {
            super::run(GatesArgs {
                path: Some(dir.path().to_path_buf()),
                run,
                json: true,
                lang: "en".into(),
            })
        };
        assert_eq!(code(true), crate::exit::FINDINGS);
        assert_eq!(code(false), crate::exit::OK);
    }
}
