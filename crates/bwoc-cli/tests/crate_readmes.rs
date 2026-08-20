//! Repo convention gate: **every crate carries a README.md**.
//!
//! A stated convention with nothing enforcing it rots — the framework has been
//! bitten twice by exactly that (the root README sat two releases stale, and
//! `bwoc-harness` was missing from every release archive because packaging was
//! never asserted). This test is the cheap enforcement: add a crate without a
//! README, or drop the `readme` manifest key, and the suite goes red.
//!
//! Two things are checked per `crates/<name>/`:
//!   1. `README.md` exists and is non-trivial (not an empty placeholder).
//!   2. `Cargo.toml` declares `readme = "README.md"`, so the file actually
//!      ships in `cargo package` / renders on a registry rather than being a
//!      repo-only courtesy.

use std::fs;
use std::path::{Path, PathBuf};

/// The workspace `crates/` directory, resolved from this crate's manifest.
fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/bwoc-cli has a parent")
        .to_path_buf()
}

/// Every directory under `crates/` that is a real crate (has a `Cargo.toml`).
fn crate_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(crates_dir())
        .expect("crates/ is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    out.sort();
    assert!(!out.is_empty(), "found no crates under crates/");
    out
}

#[test]
fn every_crate_has_a_readme() {
    let mut missing = Vec::new();
    let mut trivial = Vec::new();
    for dir in crate_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let readme = dir.join("README.md");
        match fs::read_to_string(&readme) {
            Err(_) => missing.push(name),
            // A stub that exists only to satisfy the check is not a README.
            // The real ones run 40-80 lines; 10 is a generous floor.
            Ok(body) if body.lines().filter(|l| !l.trim().is_empty()).count() < 10 => {
                trivial.push(name)
            }
            Ok(_) => {}
        }
    }
    assert!(
        missing.is_empty(),
        "these crates have no README.md (repo convention: every crate carries one): {missing:?}"
    );
    assert!(
        trivial.is_empty(),
        "these crate READMEs are too thin to be useful — write real content: {trivial:?}"
    );
}

#[test]
fn every_crate_manifest_declares_its_readme() {
    let mut undeclared = Vec::new();
    for dir in crate_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).expect("crate has a Cargo.toml");
        // Match the key at line start so a `readme` word inside a description
        // or dependency block cannot satisfy the gate.
        let declared = manifest
            .lines()
            .any(|l| l.trim_start().starts_with("readme") && l.contains("README.md"));
        if !declared {
            undeclared.push(name);
        }
    }
    assert!(
        undeclared.is_empty(),
        "these crate manifests are missing `readme = \"README.md\"` — without it the file \
         does not ship in `cargo package`: {undeclared:?}"
    );
}
