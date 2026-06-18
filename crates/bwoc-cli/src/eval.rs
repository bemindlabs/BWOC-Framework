//! `bwoc eval <fixture>` — run one eval fixture through `bwoc-harness` and
//! report the score.
//!
//! A thin front to `bwoc-harness --eval`, which owns the provider construction
//! and the scoring runner (`bwoc_harness::eval::run_fixture`). This resolves the
//! sibling binary, forwards the backend/model selection (the `--backend`
//! plumbing the eval framework lacked), runs the fixture in an **isolated** work
//! dir, and relays the harness exit code: `0` = pass or skip, `1` = fail.
//!
//! Dep-quarantine: like `bwoc spawn`, the heavy provider/runtime stack stays in
//! `bwoc-harness` — `bwoc` only spawns it.

use std::path::PathBuf;
use std::process::Command;

pub struct EvalArgs {
    /// The fixture directory (holds `fixture.toml` + optional `seed/` / `expected/`).
    pub fixture: PathBuf,
    /// Provider backend forwarded to the harness (`ollama` / `openai-compatible`
    /// / `claude` / `openrouter` / `cli`). `None` ⇒ the harness default.
    pub backend: Option<String>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    /// Work dir the fixture is seeded into. `None` ⇒ a fresh temp dir so a
    /// tool-requiring fixture's writes don't litter the cwd.
    pub workdir: Option<PathBuf>,
    pub json: bool,
}

pub fn run(args: EvalArgs) -> i32 {
    let Some(harness) = bwoc_core::exec::sibling_binary("bwoc-harness") else {
        eprintln!(
            "bwoc eval: `bwoc-harness` binary not found (install it / add it to PATH). \
             The eval runner + provider stack live there; `bwoc` only spawns it."
        );
        return 1;
    };

    let workdir = match &args.workdir {
        Some(w) => w.clone(),
        None => {
            // Unique per process so concurrent `bwoc eval` runs don't collide.
            let d = std::env::temp_dir().join(format!("bwoc-eval-{}", std::process::id()));
            if let Err(e) = std::fs::create_dir_all(&d) {
                eprintln!("bwoc eval: cannot create work dir {}: {e}", d.display());
                return 1;
            }
            d
        }
    };

    let mut cmd = Command::new(&harness);
    cmd.arg("--eval")
        .arg(&args.fixture)
        .arg("--workdir")
        .arg(&workdir);
    if let Some(b) = &args.backend {
        cmd.arg("--backend").arg(b);
    }
    if let Some(m) = &args.model {
        cmd.arg("--model").arg(m);
    }
    if let Some(e) = &args.endpoint {
        cmd.arg("--endpoint").arg(e);
    }
    if args.json {
        cmd.arg("--json");
    }

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("bwoc eval: failed to run {}: {e}", harness.display());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_harness_is_a_clean_error() {
        // With no sibling/PATH `bwoc-harness` in the unit-test environment the
        // command resolves to None and returns exit 1 rather than panicking.
        // (When a harness IS present this test is a no-op assertion on the type.)
        if bwoc_core::exec::sibling_binary("bwoc-harness").is_none() {
            let code = run(EvalArgs {
                fixture: PathBuf::from("/nonexistent/fixture"),
                backend: None,
                model: None,
                endpoint: None,
                workdir: None,
                json: false,
            });
            assert_eq!(code, 1);
        }
    }
}
