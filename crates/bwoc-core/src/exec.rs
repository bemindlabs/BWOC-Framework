//! Resolve sibling BWOC binaries relative to the *running* executable.
//!
//! Lives in `bwoc-core` so every crate that re-invokes another BWOC binary
//! in a child process (`bwoc-cli` spawning `bwoc-agent`; `bwoc-harness` and
//! `bwoc-agent` shelling out to `bwoc`) shares one resolution rule and one
//! implementation — no per-crate copies that drift.
//!
//! The rule, in priority order:
//!   1. A file **next to the currently-running binary** (`current_exe()`'s
//!      directory). A dev build or a non-`PATH` install therefore launches its
//!      *own* siblings, never a stale copy that happens to be first on `$PATH`.
//!   2. `CARGO_BIN_EXE_<name>` — set by Cargo for workspace binaries during
//!      `cargo test`, so integration tests resolve the just-built artifact.
//!   3. `$PATH` lookup — the last resort for an installed layout where the
//!      caller's `current_exe()` is somewhere unrelated.
//!
//! This is pure `std` (no new dependencies) — safe for the lean core crate.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Existing-file candidate for `name` inside `dir`. On Windows an executable is
/// `<name>.exe`, so when `name` carries no extension we also probe the `.exe`
/// sibling — otherwise a direct `is_file()` check (tiers 1 and 3) would miss a
/// Windows dev build and silently fall through to a `$PATH` copy, the very bug
/// this module exists to prevent. (The bare-name fallback in [`binary_or_name`]
/// still works on Windows because the OS appends `.exe` during a `$PATH`
/// lookup; these tiers are direct file checks, so they can't rely on that.)
fn resolve_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    if cfg!(windows) && Path::new(name).extension().is_none() {
        let exe = dir.join(format!("{name}.exe"));
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

/// Resolve a sibling BWOC binary (`bwoc`, `bwoc-agent`, `bwoc-harness`) by the
/// shared three-tier rule. Returns `None` only when none of the tiers yield an
/// executable file — callers fall back to the bare `name` (a `$PATH` lookup).
pub fn sibling_binary(name: &str) -> Option<PathBuf> {
    // 1. Sibling of the running binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(hit) = resolve_in(dir, name) {
                return Some(hit);
            }
        }
    }

    // 2. Cargo test env var (`CARGO_BIN_EXE_<name>`, hyphens preserved).
    if let Ok(p) = std::env::var(format!("CARGO_BIN_EXE_{name}")) {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Some(pb);
        }
    }

    // 3. $PATH fallback.
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        if let Some(hit) = resolve_in(&dir, name) {
            return Some(hit);
        }
    }
    None
}

/// Program name for `Command::new` when re-invoking a sibling BWOC binary:
/// the resolved sibling path if [`sibling_binary`] finds one, else the bare
/// `name` as an `OsString` so the call still works via a `$PATH` lookup
/// (preserving the old behavior rather than failing hard).
pub fn binary_or_name(name: &str) -> OsString {
    sibling_binary(name)
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_or_name_falls_back_to_the_bare_name() {
        // A name that cannot exist as a sibling of the test runner nor on PATH
        // must round-trip to the bare program name (the $PATH-lookup fallback).
        let got = binary_or_name("bwoc-definitely-not-a-real-binary-xyz");
        assert_eq!(got, OsString::from("bwoc-definitely-not-a-real-binary-xyz"));
    }

    #[test]
    fn sibling_binary_finds_the_test_runner_itself() {
        // The running test binary sits next to its own file name, so resolving
        // that exact file name must hit tier 1 (sibling of current_exe).
        let exe = std::env::current_exe().expect("test runner has a path");
        let self_name = exe.file_name().expect("exe has a file name");
        let resolved = sibling_binary(&self_name.to_string_lossy())
            .expect("the running binary resolves as its own sibling");
        assert_eq!(resolved, exe);
    }
}
