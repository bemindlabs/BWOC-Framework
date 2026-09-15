//! Bare `bwoc` and `bwoc auth` as a script sees them: no TTY, a throwaway HOME,
//! no provider env. Hermetic: nothing here reaches a network.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc"))
}

/// `bwoc` with a private HOME, no provider env, no update/what's-new side trips.
fn bwoc(home: &Path) -> Command {
    let mut c = Command::new(bin());
    c.current_dir(home)
        .env("HOME", home)
        .env("BWOC_NO_UPDATE_CHECK", "1")
        .env("BWOC_NO_WHATSNEW", "1");
    for var in [
        "BWOC_BACKEND",
        "BWOC_MODEL",
        "BWOC_ENDPOINT",
        "BWOC_LANG",
        "ANTHROPIC_API_KEY",
        "OPENROUTER_API_KEY",
        "LITELLM_API_KEY",
        "SECRET_FOR_TEST",
    ] {
        c.env_remove(var);
    }
    c.stdin(Stdio::null());
    c
}

fn run_with_stdin(mut cmd: Command, input: &str) -> Output {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn bwoc");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn bare_bwoc_without_a_tty_prints_the_banner_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let bare = bwoc(home.path()).output().unwrap();
    assert!(bare.status.success(), "{bare:?}");
    assert!(
        bare.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&bare.stderr)
    );

    let out = String::from_utf8(bare.stdout.clone()).unwrap();
    assert!(out.starts_with("\n██████╗ ██╗    ██╗"), "{out}");
    assert!(out.contains(&format!("BWOC Framework v{}", env!("CARGO_PKG_VERSION"))));
    assert!(out.contains("Available Commands:"));
    assert!(!out.contains('\x1b'), "no colour off a TTY");
    assert!(
        !out.contains("model provider"),
        "no session attempt off a TTY"
    );

    // Provider env must not change what a script sees.
    let with_env = bwoc(home.path())
        .env("BWOC_BACKEND", "ollama")
        .env("BWOC_MODEL", "m")
        .output()
        .unwrap();
    assert_eq!(with_env.stdout, bare.stdout);
    assert!(with_env.status.success());

    // `bwoc about` is the same banner, reachable on a terminal too.
    let about = bwoc(home.path()).arg("about").output().unwrap();
    assert!(about.status.success());
    assert_eq!(about.stdout, bare.stdout);
}

#[test]
fn session_flags_need_a_terminal_and_no_subcommand() {
    let home = tempfile::tempdir().unwrap();
    let no_tty = bwoc(home.path()).args(["--model", "m"]).output().unwrap();
    assert_eq!(no_tty.status.code(), Some(2));
    assert!(no_tty.stdout.is_empty());
    assert!(String::from_utf8_lossy(&no_tty.stderr).contains("need a terminal"));

    let with_sub = bwoc(home.path())
        .args(["--backend", "ollama", "about"])
        .output()
        .unwrap();
    assert_eq!(with_sub.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&with_sub.stderr).contains("only to bare `bwoc`"));
}

#[test]
fn auth_set_from_stdin_writes_0600_and_never_echoes() {
    let home = tempfile::tempdir().unwrap();
    let key = "sk-test-NEVER-ECHO-1234";
    let mut cmd = bwoc(home.path());
    cmd.args(["auth", "set", "anthropic"]);
    let out = run_with_stdin(cmd, &format!("{key}\n"));
    assert!(out.status.success(), "{out:?}");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("NEVER-ECHO"), "{all}");

    let path = home.path().join(".bwoc/secrets.toml");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(std::fs::read_to_string(&path).unwrap().contains(key));

    let status = bwoc(home.path()).args(["auth", "status"]).output().unwrap();
    assert!(status.status.success());
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(text.contains("anthropic          secrets.toml"), "{text}");
    assert!(text.contains("openrouter         not set"), "{text}");
    assert!(!text.contains("NEVER-ECHO"));
}

#[test]
fn auth_set_from_env_and_refusal_of_a_readable_file() {
    let home = tempfile::tempdir().unwrap();
    let ok = bwoc(home.path())
        .args(["auth", "set", "openrouter", "--from-env", "SECRET_FOR_TEST"])
        .env("SECRET_FOR_TEST", "sk-or-FROM-ENV")
        .output()
        .unwrap();
    assert!(ok.status.success(), "{ok:?}");
    assert!(!String::from_utf8_lossy(&ok.stdout).contains("FROM-ENV"));

    let unset = bwoc(home.path())
        .args(["auth", "set", "openrouter", "--from-env", "SECRET_FOR_TEST"])
        .output()
        .unwrap();
    assert_eq!(unset.status.code(), Some(2));

    let path = home.path().join(".bwoc/secrets.toml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut cmd = bwoc(home.path());
    cmd.args(["auth", "set", "litellm"]);
    let refused = run_with_stdin(cmd, "sk-lite\n");
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("chmod 600"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
