//! `bwoc doctor` — environment + workspace diagnostic with optional auto-fix.
//!
//! Runs a set of checks, reports each as PASS / WARN / FAIL / FIXED, and
//! exits 0 if no FAILs remain after the (optional) auto-fix pass.
//!
//! Checks are intentionally narrow and predictable. Auto-fix only touches
//! things with one obvious correct answer (missing dirs, missing
//! symlinks). Anything ambiguous — malformed config, missing AGENTS.md,
//! missing backend CLI — is reported, never silently rewritten.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use bwoc_core::workspace::{AgentEntry, AgentsRegistry, Workspace};

use crate::user_home;

pub struct DoctorArgs {
    pub path: Option<PathBuf>,
    pub auto: bool,
    /// Emit a structured JSON report instead of the human-readable list.
    /// Stable shape for CI gating and external monitoring.
    pub json: bool,
}

#[derive(Debug, Clone)]
enum Status {
    Pass,
    Warn(String),
    Fail(String),
    Fixed(String),
}

struct CheckResult {
    name: String,
    status: Status,
}

/// Standard scaffold dirs created by `bwoc init`. Mirror of
/// `init.rs::WORKSPACE_EXTRAS`. Kept local to avoid cross-module
/// re-export ceremony.
const WORKSPACE_EXTRAS: &[&str] = &["projects", "notes"];
/// Backend symlink files inside each incarnated agent. Mirror of
/// `new.rs::create_symlinks`.
const BACKEND_SYMLINKS: &[&str] = &["CLAUDE.md", "AGY.md", "CODEX.md", "KIMI.md", "OLLAMA.md"];

pub fn run(args: DoctorArgs) -> i32 {
    let mut results: Vec<CheckResult> = Vec::new();

    // 1. ~/.bwoc/ directory.
    results.push(check_user_home(args.auto));

    // 2. Backend CLIs on PATH (informational).
    results.push(check_backends());

    // 2a. Rust toolchain (rustup + cargo) — informational. Needed for the
    // four mandatory gates (`cargo build/test/clippy/fmt`) and for
    // `cargo install --path crates/bwoc-harness` (the ollama harness).
    for r in check_rust_toolchain() {
        results.push(r);
    }

    // 2b. Ollama-specific: bwoc-harness binary + endpoint reachability.
    for r in check_ollama() {
        results.push(r);
    }

    // 3. Workspace-level checks if we're inside one.
    let ws_root = resolve_workspace_root(args.path);
    match ws_root {
        Some(root) => {
            results.push(check_workspace_toml(&root));
            results.push(check_agents_toml(&root));
            results.push(check_scaffold_dirs(&root, args.auto));
            // Per-agent symlinks (only if registry parsed).
            if AgentsRegistry::load(&root).is_ok() {
                for r in check_agent_symlinks(&root, args.auto) {
                    results.push(r);
                }
                // Stale `agent.pid` cleanup (foundation laid by
                // `bwoc-agent --serve`).
                for r in check_stale_pids(&root, args.auto) {
                    results.push(r);
                }
                // Stale `agent.sock` cleanup (sibling of the PID sweep).
                for r in check_stale_sockets(&root, args.auto) {
                    results.push(r);
                }
                // Inbox cursor sanity (corrupt or out-of-bounds files).
                for r in check_inbox_cursors(&root, args.auto) {
                    results.push(r);
                }
                // Per-agent daemon log size — append-only, can grow unbounded
                // across many start/stop cycles. WARN when bloated; --auto
                // truncates in place.
                for r in check_oversize_logs(&root, args.auto) {
                    results.push(r);
                }
                // Per-agent inbox size — WARN only. Inbox content is user
                // data, not diagnostic chatter; --auto must NOT discard it
                // silently. The user clears it manually via `bwoc inbox --clear`.
                for r in check_oversize_inboxes(&root) {
                    results.push(r);
                }
            }
        }
        None => results.push(CheckResult {
            name: "workspace".into(),
            status: Status::Warn(
                "not inside a BWOC workspace — workspace-level checks skipped. \
                 Pass a path or run `bwoc init` first."
                    .into(),
            ),
        }),
    }

    if args.json {
        emit_json(&results);
    } else {
        print_report(&results, args.auto);
    }

    if results.iter().any(|r| matches!(r.status, Status::Fail(_))) {
        2
    } else {
        0
    }
}

/// Stable JSON shape:
/// ```text
/// {
///   "results": [
///     { "name": "<check name>", "status": "pass"|"warn"|"fail"|"fixed",
///       "detail": "<message or null>" },
///     ...
///   ],
///   "summary": { "pass": N, "warn": N, "fail": N, "fixed": N },
///   "exit": 0 | 2     // 2 iff any "fail" present
/// }
/// ```
fn emit_json(results: &[CheckResult]) {
    let mut pass = 0u32;
    let mut warn = 0u32;
    let mut fail = 0u32;
    let mut fixed = 0u32;
    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let (status, detail) = match &r.status {
                Status::Pass => {
                    pass += 1;
                    ("pass", None)
                }
                Status::Warn(m) => {
                    warn += 1;
                    ("warn", Some(m.as_str()))
                }
                Status::Fail(m) => {
                    fail += 1;
                    ("fail", Some(m.as_str()))
                }
                Status::Fixed(m) => {
                    fixed += 1;
                    ("fixed", Some(m.as_str()))
                }
            };
            serde_json::json!({
                "name": r.name,
                "status": status,
                "detail": detail,
            })
        })
        .collect();
    let value = serde_json::json!({
        "results": items,
        "summary": { "pass": pass, "warn": warn, "fail": fail, "fixed": fixed },
        "exit": if fail > 0 { 2 } else { 0 },
    });
    match serde_json::to_string_pretty(&value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("bwoc doctor --json: serialize failed: {e}"),
    }
}

// --- individual checks -----------------------------------------------------

fn check_user_home(auto: bool) -> CheckResult {
    match user_home::bwoc_home() {
        Ok(p) if p.is_dir() => CheckResult {
            name: "~/.bwoc/".into(),
            status: Status::Pass,
        },
        Ok(p) if auto => match user_home::ensure_initialized() {
            Ok(_) => CheckResult {
                name: "~/.bwoc/".into(),
                status: Status::Fixed(format!("created {}", p.display())),
            },
            Err(e) => CheckResult {
                name: "~/.bwoc/".into(),
                status: Status::Fail(format!("could not create: {e}")),
            },
        },
        Ok(p) => CheckResult {
            name: "~/.bwoc/".into(),
            status: Status::Fail(format!(
                "missing — rerun any bwoc command or pass --auto to create at {}",
                p.display()
            )),
        },
        Err(e) => CheckResult {
            name: "~/.bwoc/".into(),
            status: Status::Fail(format!("cannot resolve: {e}")),
        },
    }
}

fn check_backends() -> CheckResult {
    // Vendor CLIs (external programs on PATH).
    let vendor_backends = ["claude", "agy", "codex", "kimi"];
    let mut available = Vec::new();
    for b in vendor_backends {
        if which(b).is_some() {
            available.push(b);
        }
    }
    let name = "backends on PATH".to_string();
    if available.is_empty() {
        CheckResult {
            name,
            status: Status::Warn(
                "no backend CLI on PATH (claude/agy/codex/kimi). `bwoc spawn` will fail.".into(),
            ),
        }
    } else {
        CheckResult {
            name,
            status: Status::Pass,
        }
    }
}

/// Check the Ollama backend: verify `bwoc-harness` is reachable and that
/// the Ollama endpoint responds.  Both are informational (WARN not FAIL) —
/// the user may not intend to use Ollama at all.
fn check_ollama() -> Vec<CheckResult> {
    let mut out = Vec::new();

    // 1. bwoc-harness binary availability.
    let harness_result = match crate::spawn::Backend::harness_binary() {
        Some(_path) => CheckResult {
            name: "bwoc-harness binary".into(),
            status: Status::Pass,
        },
        None => CheckResult {
            name: "bwoc-harness binary".into(),
            status: Status::Warn(
                "bwoc-harness not found (sibling dir / PATH). \
                 Install with `cargo install --path crates/bwoc-harness` \
                 to use the ollama backend."
                    .into(),
            ),
        },
    };
    out.push(harness_result);

    // 2. Ollama endpoint reachability (TCP connect to localhost:11434).
    //    We use only std::net — no HTTP dep — to keep bwoc-cli lean.
    let endpoint_result = {
        use std::net::TcpStream;
        use std::time::Duration;
        let addr = "127.0.0.1:11434";
        let reachable =
            TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(500)).is_ok();
        if reachable {
            CheckResult {
                name: "ollama endpoint (localhost:11434)".into(),
                status: Status::Pass,
            }
        } else {
            CheckResult {
                name: "ollama endpoint (localhost:11434)".into(),
                status: Status::Warn(
                    "Ollama not reachable at localhost:11434. \
                     Start with `ollama serve` to use the ollama backend."
                        .into(),
                ),
            }
        }
    };
    out.push(endpoint_result);

    out
}

/// Probe the Rust toolchain: `rustup` and `cargo`. Contributors need both
/// for the four mandatory gates (`cargo build/test/clippy/fmt`) and for
/// `cargo install --path crates/bwoc-harness` (the ollama harness path
/// already hinted by `check_ollama`). Both checks are informational
/// (WARN, never FAIL) — users who only `bwoc spawn` against a vendor
/// backend (claude/agy/codex/kimi) don't need the Rust toolchain. Mirrors
/// the WARN policy of `check_backends` and the ollama probes.
///
/// Read-only invariant: the only commands issued are `rustup --version`
/// and `cargo --version`. The probe never installs, updates, sets a
/// default toolchain, or writes to `~/.rustup/` / `~/.cargo/`.
fn check_rust_toolchain() -> Vec<CheckResult> {
    check_rust_toolchain_with(which)
}

/// Inner form with an injectable presence probe so unit tests can force
/// the "not on PATH" branch without mutating the global `PATH`.
fn check_rust_toolchain_with(which_fn: impl Fn(&str) -> Option<PathBuf>) -> Vec<CheckResult> {
    vec![
        probe_toolchain(
            "rustup",
            &which_fn,
            "rustup not found on PATH. Contributors need it for \
             `cargo build/test/clippy/fmt`. Install: https://rustup.rs",
        ),
        probe_toolchain(
            "cargo",
            &which_fn,
            "cargo not found on PATH. Required to build BWOC from source \
             or `cargo install bwoc-harness`. Install: https://rustup.rs",
        ),
    ]
}

/// Single toolchain probe. Absent on PATH → WARN with the install hint.
/// Present but `--version` fails to spawn or exits non-zero → WARN with a
/// diagnostic snippet. Present and `--version` succeeds → PASS.
fn probe_toolchain(
    tool: &str,
    which_fn: &impl Fn(&str) -> Option<PathBuf>,
    missing_detail: &str,
) -> CheckResult {
    if which_fn(tool).is_none() {
        return CheckResult {
            name: tool.into(),
            status: Status::Warn(missing_detail.into()),
        };
    }
    // Present on PATH — confirm `--version` runs cleanly. Read-only: the
    // single arg is always `--version`, never a mutating subcommand.
    match Command::new(tool).arg("--version").output() {
        Ok(out) if out.status.success() => CheckResult {
            name: tool.into(),
            status: Status::Pass,
        },
        Ok(out) => {
            let raw = if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            };
            let snippet = String::from_utf8_lossy(raw);
            let snippet = snippet.lines().next().unwrap_or("").trim();
            CheckResult {
                name: tool.into(),
                status: Status::Warn(format!(
                    "{tool} present but `--version` returned non-zero: {snippet}"
                )),
            }
        }
        Err(e) => CheckResult {
            name: tool.into(),
            status: Status::Warn(format!("{tool} present but `--version` could not run: {e}")),
        },
    }
}

fn check_workspace_toml(root: &Path) -> CheckResult {
    match Workspace::load(root) {
        Ok(_) => CheckResult {
            name: ".bwoc/workspace.toml".into(),
            status: Status::Pass,
        },
        Err(e) => CheckResult {
            name: ".bwoc/workspace.toml".into(),
            status: Status::Fail(format!("parse error: {e}")),
        },
    }
}

fn check_agents_toml(root: &Path) -> CheckResult {
    match AgentsRegistry::load(root) {
        Ok(_) => CheckResult {
            name: ".bwoc/agents.toml".into(),
            status: Status::Pass,
        },
        Err(e) => CheckResult {
            name: ".bwoc/agents.toml".into(),
            status: Status::Fail(format!("parse error: {e}")),
        },
    }
}

fn check_scaffold_dirs(root: &Path, auto: bool) -> CheckResult {
    let ws = Workspace::load(root).ok();
    let agents_dir = ws
        .map(|w| w.defaults.agents_dir)
        .unwrap_or_else(|| "agents".into());
    let mut to_check = vec![agents_dir.as_str()];
    to_check.extend(WORKSPACE_EXTRAS.iter().copied());

    let missing: Vec<&str> = to_check
        .into_iter()
        .filter(|d| !root.join(d).is_dir())
        .collect();

    let name = "scaffold dirs".into();
    if missing.is_empty() {
        return CheckResult {
            name,
            status: Status::Pass,
        };
    }
    if auto {
        let mut created = Vec::new();
        for d in &missing {
            if fs::create_dir_all(root.join(d)).is_ok() {
                created.push(*d);
            }
        }
        return CheckResult {
            name,
            status: Status::Fixed(format!("created: {}", created.join(", "))),
        };
    }
    CheckResult {
        name,
        status: Status::Fail(format!(
            "missing: {} (rerun with --auto to create)",
            missing.join(", ")
        )),
    }
}

/// Detect `<agent>/.bwoc/agent.pid` files whose process is no longer
/// alive (signal-0 fails). With --auto, remove the stale file.
///
/// Mirrors the liveness check in `status.rs::signal_zero_alive`;
/// duplicated to keep the doctor module self-contained (extract to a
/// shared helper when a third caller appears).
fn check_stale_pids(root: &Path, auto: bool) -> Vec<CheckResult> {
    let Ok(registry) = AgentsRegistry::load(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for a in &registry.agents {
        let pid_path = root.join(&a.path).join(".bwoc/agent.pid");
        if !pid_path.is_file() {
            continue; // Not a daemon-mode agent; nothing to clean.
        }
        let raw = match std::fs::read_to_string(&pid_path) {
            Ok(s) => s,
            Err(e) => {
                out.push(CheckResult {
                    name: format!("agent pid: {}", a.id),
                    status: Status::Warn(format!("could not read {}: {e}", pid_path.display())),
                });
                continue;
            }
        };
        let pid: u32 = match raw.trim().parse() {
            Ok(n) => n,
            Err(_) => {
                // Malformed PID file — auto-fix would remove it.
                if auto {
                    let _ = std::fs::remove_file(&pid_path);
                    out.push(CheckResult {
                        name: format!("agent pid: {}", a.id),
                        status: Status::Fixed(format!(
                            "removed malformed PID file: {}",
                            pid_path.display()
                        )),
                    });
                } else {
                    out.push(CheckResult {
                        name: format!("agent pid: {}", a.id),
                        status: Status::Fail(format!(
                            "malformed PID file at {} (rerun with --auto to remove)",
                            pid_path.display()
                        )),
                    });
                }
                continue;
            }
        };
        if crate::livecheck::signal_zero_alive(pid) {
            // Process exists — not stale, skip silently.
            continue;
        }
        // Stale.
        if auto {
            if std::fs::remove_file(&pid_path).is_ok() {
                out.push(CheckResult {
                    name: format!("agent pid: {}", a.id),
                    status: Status::Fixed(format!("removed stale PID file (pid {pid} not alive)")),
                });
            } else {
                out.push(CheckResult {
                    name: format!("agent pid: {}", a.id),
                    status: Status::Fail(format!(
                        "stale PID {pid} but couldn't remove {}",
                        pid_path.display()
                    )),
                });
            }
        } else {
            out.push(CheckResult {
                name: format!("agent pid: {}", a.id),
                status: Status::Fail(format!(
                    "stale PID file at {} (pid {pid} not alive; rerun with --auto to remove)",
                    pid_path.display()
                )),
            });
        }
    }
    out
}

// signal_zero_alive moved to crate::livecheck.

/// Detect `<agent>/.bwoc/agent.sock` files that no live process owns.
/// A socket is considered stale when ANY of:
///   - sibling `agent.pid` missing  (orphan socket from a crash)
///   - sibling `agent.pid` exists but the pid isn't alive
///
/// With `--auto`, remove the stale socket. Live sockets are left alone.
fn check_stale_sockets(root: &Path, auto: bool) -> Vec<CheckResult> {
    let Ok(registry) = AgentsRegistry::load(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for a in &registry.agents {
        let bwoc = root.join(&a.path).join(".bwoc");
        let sock_path = bwoc.join("agent.sock");
        if !sock_path.exists() {
            continue;
        }
        // Is there a live owning process?
        let pid_path = bwoc.join("agent.pid");
        let owner_alive = std::fs::read_to_string(&pid_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map(crate::livecheck::signal_zero_alive)
            .unwrap_or(false);
        if owner_alive {
            continue; // Socket has a live owner; not stale.
        }
        // Stale.
        if auto {
            if std::fs::remove_file(&sock_path).is_ok() {
                out.push(CheckResult {
                    name: format!("agent sock: {}", a.id),
                    status: Status::Fixed(
                        "removed stale socket (no live owning process)".to_string(),
                    ),
                });
            } else {
                out.push(CheckResult {
                    name: format!("agent sock: {}", a.id),
                    status: Status::Fail(format!(
                        "stale socket at {} but couldn't remove",
                        sock_path.display()
                    )),
                });
            }
        } else {
            out.push(CheckResult {
                name: format!("agent sock: {}", a.id),
                status: Status::Fail(format!(
                    "stale socket at {} (no live owner; rerun with --auto to remove)",
                    sock_path.display()
                )),
            });
        }
    }
    out
}

/// Detect `<agent>/.bwoc/inbox.cursor` files that are corrupt or out of
/// bounds. Sister sweep to `check_stale_pids` / `check_stale_sockets`.
///
/// Three failure modes:
///   - Malformed (won't parse as u64) → FAIL; --auto removes the file
///   - Cursor > inbox file size → FAIL; --auto removes the file
///   - Cursor present + inbox file missing → FAIL; --auto removes
///
/// Out-of-bounds cursors are also handled by the daemon's own
/// truncation-recovery at startup, but doctor surfacing them makes the
/// inconsistency visible to the human.
///
/// Live, in-bounds cursors are silently OK.
fn check_inbox_cursors(root: &Path, auto: bool) -> Vec<CheckResult> {
    let Ok(registry) = AgentsRegistry::load(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for a in &registry.agents {
        let bwoc = root.join(&a.path).join(".bwoc");
        let cursor_path = bwoc.join("inbox.cursor");
        if !cursor_path.is_file() {
            continue; // No cursor yet; nothing to check.
        }
        let raw = match std::fs::read_to_string(&cursor_path) {
            Ok(s) => s,
            Err(e) => {
                out.push(CheckResult {
                    name: format!("inbox cursor: {}", a.id),
                    status: Status::Warn(format!("could not read {}: {e}", cursor_path.display())),
                });
                continue;
            }
        };
        let pos: u64 = match raw.trim().parse() {
            Ok(n) => n,
            Err(_) => {
                if auto {
                    let _ = std::fs::remove_file(&cursor_path);
                    out.push(CheckResult {
                        name: format!("inbox cursor: {}", a.id),
                        status: Status::Fixed(format!(
                            "removed malformed cursor file: {}",
                            cursor_path.display()
                        )),
                    });
                } else {
                    out.push(CheckResult {
                        name: format!("inbox cursor: {}", a.id),
                        status: Status::Fail(format!(
                            "malformed cursor file at {} (rerun with --auto to remove)",
                            cursor_path.display()
                        )),
                    });
                }
                continue;
            }
        };
        let inbox_path = bwoc.join("inbox.jsonl");
        let size = std::fs::metadata(&inbox_path).map(|m| m.len()).ok();
        match size {
            None => {
                // Cursor exists but inbox file doesn't — orphan cursor.
                if auto {
                    if std::fs::remove_file(&cursor_path).is_ok() {
                        out.push(CheckResult {
                            name: format!("inbox cursor: {}", a.id),
                            status: Status::Fixed(
                                "removed orphan cursor (no inbox.jsonl)".to_string(),
                            ),
                        });
                    } else {
                        out.push(CheckResult {
                            name: format!("inbox cursor: {}", a.id),
                            status: Status::Fail(format!(
                                "orphan cursor at {} but couldn't remove",
                                cursor_path.display()
                            )),
                        });
                    }
                } else {
                    out.push(CheckResult {
                        name: format!("inbox cursor: {}", a.id),
                        status: Status::Fail(format!(
                            "cursor at {} but inbox.jsonl missing (rerun with --auto to remove)",
                            cursor_path.display()
                        )),
                    });
                }
            }
            Some(sz) if pos > sz => {
                // Out-of-bounds cursor.
                if auto {
                    if std::fs::remove_file(&cursor_path).is_ok() {
                        out.push(CheckResult {
                            name: format!("inbox cursor: {}", a.id),
                            status: Status::Fixed(format!(
                                "removed out-of-bounds cursor ({pos} > inbox size {sz}) — daemon will reset to EOF"
                            )),
                        });
                    } else {
                        out.push(CheckResult {
                            name: format!("inbox cursor: {}", a.id),
                            status: Status::Fail(format!(
                                "out-of-bounds cursor at {} (cursor {pos} > size {sz}) but couldn't remove",
                                cursor_path.display()
                            )),
                        });
                    }
                } else {
                    out.push(CheckResult {
                        name: format!("inbox cursor: {}", a.id),
                        status: Status::Fail(format!(
                            "cursor {pos} > inbox size {sz} at {} (rerun with --auto to remove)",
                            cursor_path.display()
                        )),
                    });
                }
            }
            Some(_) => {
                // In bounds — silently OK.
            }
        }
    }
    out
}

/// Detect `<agent>/.bwoc/agent.log` files that have grown past
/// `LOG_BLOAT_BYTES`. Logs are append-mode and survive across
/// `bwoc start` / `bwoc stop` cycles; for a long-lived agent this
/// can accumulate megabytes of diagnostic chatter that's mostly
/// duplicate startup banners.
///
/// Without `--auto`, emits WARN with a hint pointing at
/// `bwoc log --clear`. With `--auto`, truncates in place
/// (preserves inode + daemon's open handle keeps writing).
const LOG_BLOAT_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB — generous

fn check_oversize_logs(root: &Path, auto: bool) -> Vec<CheckResult> {
    let Ok(registry) = AgentsRegistry::load(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for a in &registry.agents {
        let log_path = root.join(&a.path).join(".bwoc/agent.log");
        let Ok(meta) = std::fs::metadata(&log_path) else {
            continue; // No log yet — nothing to check.
        };
        let size = meta.len();
        if size <= LOG_BLOAT_BYTES {
            continue;
        }
        let size_mib = size as f64 / (1024.0 * 1024.0);
        if auto {
            match std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&log_path)
            {
                Ok(_) => out.push(CheckResult {
                    name: format!("agent log: {}", a.id),
                    status: Status::Fixed(format!(
                        "truncated {} (was {size_mib:.1} MiB)",
                        log_path.display(),
                    )),
                }),
                Err(e) => out.push(CheckResult {
                    name: format!("agent log: {}", a.id),
                    status: Status::Fail(format!(
                        "log at {} is {size_mib:.1} MiB but couldn't truncate: {e}",
                        log_path.display()
                    )),
                }),
            }
        } else {
            out.push(CheckResult {
                name: format!("agent log: {}", a.id),
                status: Status::Warn(format!(
                    "{size_mib:.1} MiB at {} — `bwoc log {} --clear` to truncate, or rerun doctor with --auto",
                    log_path.display(),
                    a.id.strip_prefix("agent-").unwrap_or(&a.id),
                )),
            });
        }
    }
    out
}

/// Detect `<agent>/.bwoc/inbox.jsonl` files that have grown past
/// `INBOX_BLOAT_BYTES`. Unlike `agent.log` (diagnostic chatter,
/// truncatable by --auto), the inbox is **user data** — silently
/// discarding it would lose messages. So this check is WARN-only;
/// the fix is the user-driven `bwoc inbox <agent> --clear`.
const INBOX_BLOAT_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB

fn check_oversize_inboxes(root: &Path) -> Vec<CheckResult> {
    let Ok(registry) = AgentsRegistry::load(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for a in &registry.agents {
        let inbox_path = root.join(&a.path).join(".bwoc/inbox.jsonl");
        let Ok(meta) = std::fs::metadata(&inbox_path) else {
            continue;
        };
        let size = meta.len();
        if size <= INBOX_BLOAT_BYTES {
            continue;
        }
        let size_mib = size as f64 / (1024.0 * 1024.0);
        let bare = a.id.strip_prefix("agent-").unwrap_or(&a.id);
        out.push(CheckResult {
            name: format!("agent inbox: {}", a.id),
            // Inbox is user data — do NOT offer --auto. Point at the
            // user-driven `bwoc inbox --clear` instead.
            status: Status::Warn(format!(
                "{size_mib:.1} MiB at {} — `bwoc inbox {bare} --clear` to drain (user data; doctor will not auto-discard)",
                inbox_path.display(),
            )),
        });
    }
    out
}

fn check_agent_symlinks(root: &Path, auto: bool) -> Vec<CheckResult> {
    let Ok(registry) = AgentsRegistry::load(root) else {
        return vec![];
    };
    registry
        .agents
        .iter()
        .map(|a| check_single_agent(root, a, auto))
        .collect()
}

fn check_single_agent(root: &Path, a: &AgentEntry, auto: bool) -> CheckResult {
    let agent_path = root.join(&a.path);
    let name = format!("agent: {}", a.id);

    if !agent_path.is_dir() {
        return CheckResult {
            name,
            status: Status::Fail(format!("agent directory missing: {}", agent_path.display())),
        };
    }
    let agents_md = agent_path.join("AGENTS.md");
    if !agents_md.is_file() {
        return CheckResult {
            name,
            status: Status::Fail(format!("missing AGENTS.md in {}", agent_path.display())),
        };
    }

    let mut missing = Vec::new();
    for link in BACKEND_SYMLINKS {
        let lp = agent_path.join(link);
        if !lp.exists() {
            missing.push(*link);
        }
    }
    if missing.is_empty() {
        return CheckResult {
            name,
            status: Status::Pass,
        };
    }
    if auto {
        // Explicit element type because the only `push` site is cfg(unix);
        // on Windows the Vec would have no inferred type.
        let mut fixed: Vec<&&str> = Vec::new();
        for link in &missing {
            let _target = agent_path.join(link);
            #[cfg(unix)]
            if std::os::unix::fs::symlink("AGENTS.md", &_target).is_ok() {
                fixed.push(link);
            }
        }
        let body = fixed.iter().map(|s| **s).collect::<Vec<&str>>().join(", ");
        return CheckResult {
            name,
            status: Status::Fixed(format!("recreated symlinks: {body}")),
        };
    }
    CheckResult {
        name,
        status: Status::Fail(format!(
            "missing backend symlinks: {} (rerun with --auto to recreate)",
            missing.join(", ")
        )),
    }
}

// --- helpers ---------------------------------------------------------------

/// `which`-equivalent: returns the first PATH entry that contains an
/// executable named `cmd`. Avoids depending on the `which` crate.
fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(cmd);
        if Command::new(&candidate).arg("--version").output().is_ok() {
            return Some(candidate);
        }
    }
    None
}

/// Workspace resolution similar to workspace.rs::find_workspace_root —
/// explicit > BWOC_WORKSPACE env > ancestor walk from cwd > None.
fn resolve_workspace_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return Some(PathBuf::from(env_path));
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

fn print_report(results: &[CheckResult], auto: bool) {
    println!();
    println!("BWOC Doctor");
    println!("===========");
    if auto {
        println!("(running in --auto mode: safe issues will be fixed in place)");
    }
    println!();
    let mut pass = 0u32;
    let mut warn = 0u32;
    let mut fail = 0u32;
    let mut fixed = 0u32;
    for r in results {
        match &r.status {
            Status::Pass => {
                pass += 1;
                println!("  PASS   {}", r.name);
            }
            Status::Warn(msg) => {
                warn += 1;
                println!("  WARN   {} — {msg}", r.name);
            }
            Status::Fail(msg) => {
                fail += 1;
                println!("  FAIL   {} — {msg}", r.name);
            }
            Status::Fixed(msg) => {
                fixed += 1;
                println!("  FIXED  {} — {msg}", r.name);
            }
        }
    }
    println!();
    println!("===========\n{pass} pass · {warn} warn · {fail} fail · {fixed} fixed");
    if fail > 0 {
        println!();
        if auto {
            println!(
                "Some failures couldn't be auto-fixed (need user attention — \
                 malformed config, missing AGENTS.md, missing backend CLI)."
            );
        } else {
            println!("Run `bwoc doctor --auto` to attempt auto-fix on safe issues.");
        }
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_scaffold_dirs_reported_when_no_auto() {
        let base = std::env::temp_dir().join(format!("bwoc-doctor-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".bwoc")).unwrap();
        fs::write(
            base.join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\ncreated=\"x\"\n",
        )
        .unwrap();
        let r = check_scaffold_dirs(&base, /*auto=*/ false);
        assert!(matches!(r.status, Status::Fail(_)));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_scaffold_dirs_fixed_with_auto() {
        let base = std::env::temp_dir().join(format!("bwoc-doctor-fix-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".bwoc")).unwrap();
        fs::write(
            base.join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\ncreated=\"x\"\n",
        )
        .unwrap();
        let r = check_scaffold_dirs(&base, /*auto=*/ true);
        assert!(matches!(r.status, Status::Fixed(_)));
        assert!(base.join("agents").is_dir());
        assert!(base.join("projects").is_dir());
        assert!(base.join("notes").is_dir());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rust_toolchain_probe_returns_two_results() {
        // Inject an always-absent `which` so the probe never spawns a
        // process; we only assert the shape (count + names + order).
        let results = check_rust_toolchain_with(|_| None);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "rustup");
        assert_eq!(results[1].name, "cargo");
    }

    #[test]
    fn rust_toolchain_warns_when_rustup_missing() {
        let results = check_rust_toolchain_with(|_| None);
        match &results[0].status {
            Status::Warn(msg) => {
                assert!(msg.contains("rustup not found"), "detail: {msg}");
                assert!(msg.contains("rustup.rs"), "detail: {msg}");
            }
            other => panic!("expected Warn for missing rustup, got {other:?}"),
        }
    }

    #[test]
    fn rust_toolchain_warns_when_cargo_missing() {
        let results = check_rust_toolchain_with(|_| None);
        match &results[1].status {
            Status::Warn(msg) => {
                assert!(msg.contains("cargo not found"), "detail: {msg}");
                assert!(msg.contains("rustup.rs"), "detail: {msg}");
            }
            other => panic!("expected Warn for missing cargo, got {other:?}"),
        }
    }

    #[test]
    fn rust_toolchain_passes_on_dev_host() {
        // Skip on hosts without the toolchain (e.g. minimal CI images) —
        // the probe is WARN-not-FAIL there, which other tests cover.
        if which("rustup").is_none() || which("cargo").is_none() {
            return;
        }
        let results = check_rust_toolchain();
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(
                matches!(r.status, Status::Pass),
                "{} expected Pass, got {:?}",
                r.name,
                r.status
            );
        }
    }
}
