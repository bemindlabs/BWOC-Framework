//! `bwoc a2a` — expose a local agent over the A2A protocol (#48 P1-serve).
//!
//! This is a thin **subprocess shim**: the A2A transport (axum/tokio) lives in
//! the sibling `bwoc-a2a` binary, which this module resolves and execs. The CLI
//! deliberately does NOT link `bwoc-a2a` — that would pull tokio/hyper into
//! `bwoc-cli`'s dependency tree and break the dep-quarantine invariant
//! documented in `spawn.rs` (the same reason `bwoc spawn` runs `bwoc-harness`
//! as a subprocess rather than linking it).
//!
//! - `bwoc a2a card <agent>`  → print the agent's Agent Card JSON.
//! - `bwoc a2a serve <agent>` → run the A2A HTTP listener (loopback-only default).

use std::ffi::OsString;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::Command;

/// Args for `bwoc a2a card`.
pub struct CardArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
}

/// Args for `bwoc a2a serve`.
pub struct ServeArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
    pub bind: IpAddr,
    pub port: u16,
    pub team: Option<String>,
    pub allow_unauthenticated: bool,
}

pub fn run_card(args: CardArgs) -> i32 {
    let mut argv: Vec<OsString> = vec!["card".into(), args.agent.into()];
    push_workspace(&mut argv, args.workspace);
    exec_sibling(argv)
}

/// Args for `bwoc a2a fetch-card`.
pub struct FetchCardArgs {
    pub url: String,
}

pub fn run_fetch_card(args: FetchCardArgs) -> i32 {
    exec_sibling(vec!["fetch-card".into(), args.url.into()])
}

/// Args for `bwoc a2a send`.
pub struct SendOutboundArgs {
    pub url: String,
    pub message: String,
    pub context: Option<String>,
}

pub fn run_send_outbound(args: SendOutboundArgs) -> i32 {
    let mut argv: Vec<OsString> = vec!["send".into(), args.url.into(), args.message.into()];
    if let Some(ctx) = args.context {
        argv.push("--context".into());
        argv.push(ctx.into());
    }
    exec_sibling(argv)
}

pub fn run_serve(args: ServeArgs) -> i32 {
    let mut argv: Vec<OsString> = vec!["serve".into(), args.agent.into()];
    push_workspace(&mut argv, args.workspace);
    argv.push("--bind".into());
    argv.push(args.bind.to_string().into());
    argv.push("--port".into());
    argv.push(args.port.to_string().into());
    if let Some(team) = args.team {
        argv.push("--team".into());
        argv.push(team.into());
    }
    if args.allow_unauthenticated {
        argv.push("--allow-unauthenticated".into());
    }
    exec_sibling(argv)
}

fn push_workspace(argv: &mut Vec<OsString>, workspace: Option<PathBuf>) {
    if let Some(ws) = workspace {
        argv.push("--workspace".into());
        argv.push(ws.into_os_string());
    }
}

/// Resolve the `bwoc-a2a` sibling binary and exec it with `argv`, returning its
/// exit code. Resolution mirrors `spawn::Backend::harness_binary` — beside the
/// running `bwoc` binary, then `CARGO_BIN_EXE_bwoc-a2a` (cargo test), then `$PATH`.
fn exec_sibling(argv: Vec<OsString>) -> i32 {
    let Some(bin) = a2a_binary() else {
        eprintln!(
            "bwoc a2a: could not find the `bwoc-a2a` binary (looked beside `bwoc` \
             and on $PATH). Reinstall BWOC or ensure `bwoc-a2a` is installed \
             alongside `bwoc`."
        );
        return 127;
    };
    match Command::new(&bin).args(&argv).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("bwoc a2a: failed to run {}: {e}", bin.display());
            1
        }
    }
}

fn a2a_binary() -> Option<PathBuf> {
    const BIN: &str = "bwoc-a2a";
    // 1. Sibling of the running binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(BIN);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // 2. Cargo test env var.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_bwoc-a2a") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    // 3. $PATH fallback.
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(BIN);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
