//! Phase 5 t4 — proof that every `PURE_READ_TOOLS` entry is egress-clean.
//!
//! The whitelist [`bwoc_harness::policy::PURE_READ_TOOLS`] is the single source
//! of truth. This suite proves the *claim* that membership encodes — "this tool
//! reads and nothing else" — at two strengths, exactly as chartered:
//!
//! * **Behavioral (PRIMARY, Linux only).** Each whitelisted tool is run for real
//!   under a Landlock domain that denies *all* filesystem writes and *all* TCP
//!   egress. Before any tool runs, the domain is empirically self-tested (a write
//!   and a connect must both be refused with `EACCES`) so a no-op sandbox can
//!   never produce a false green (t4 BC-1b). The whole set is enumerated from the
//!   const with no opt-out list (t4 BC-3a).
//! * **Static floor (UNIVERSAL).** On every platform, each whitelisted tool's
//!   source (its `impl ToolImpl` block plus the same-file helpers it calls) is
//!   scanned for forbidden effect symbols (network / process / FS-write). This is
//!   a *fast floor*, not a full proof — transitive-dep taint and cross-file
//!   helpers are the behavioral test's job (t4 BC-4).
//!
//! Scope of "egress-clean" (the t4 charter): externally-observable effects only —
//! network, DNS, FS writes outside the worktree, process spawn, IPC. Wall-clock
//! timing is a known-accepted intra-turn residual, not egress.

use std::collections::HashSet;
use std::path::Path;

// ---------------------------------------------------------------------------
// Per-tool input fixtures — enumerated from the const, no opt-out (t4 BC-3a)
// ---------------------------------------------------------------------------

/// What we assert about a single tool invocation's output.
//
// The payload is consumed only by the Linux behavioral run; on other platforms
// the static/exhaustiveness tests construct `Expect` but never match the inner
// string, so silence the would-be dead-code warning there (it is genuinely live
// on Linux).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum Expect {
    /// The output must contain this substring (a happy-path read).
    Contains(&'static str),
    /// The tool merely has to *run* to completion (an edge input — absent file,
    /// out-of-confinement path, no-match). The point is that it stays egress-clean
    /// on this input too; the sandbox, not the assertion, enforces that.
    Runs,
}

/// Representative inputs for a pure-read tool (t4 BC-4: residual egress is
/// input-conditional, so we exercise match/no-match, present/absent, and
/// in/out-of-confinement paths).
///
/// **Exhaustive or panic** — a tool in `PURE_READ_TOOLS` with no entry here makes
/// this function panic, so a newly-whitelisted tool that nobody taught the proof
/// to exercise fails the suite rather than slipping through (t4 BC-3a: a tool that
/// cannot be exercised cannot be PureRead).
fn input_cases(tool: &str) -> Vec<(&'static str, Expect)> {
    match tool {
        "read_file" => vec![
            (
                r#"{"path":"note.txt"}"#,
                Expect::Contains("hello from read_file"),
            ),
            (r#"{"path":"does_not_exist.txt"}"#, Expect::Runs), // absent → error string
            (r#"{"path":"../escape.txt"}"#, Expect::Runs),      // out-of-confinement
        ],
        "list_dir" => vec![
            (r#"{}"#, Expect::Contains("note.txt")),
            (r#"{"path":"sub"}"#, Expect::Contains("code.rs")),
            (r#"{"path":"nope"}"#, Expect::Runs), // absent dir
        ],
        "grep" => vec![
            (
                r#"{"pattern":"NEEDLE_xyz"}"#,
                Expect::Contains("NEEDLE_xyz"),
            ),
            (
                r#"{"pattern":"ZZZ_absent_pattern"}"#,
                Expect::Contains("no matches"),
            ),
            (
                r#"{"pattern":"NEEDLE_xyz","path":"sub"}"#,
                Expect::Contains("code.rs"),
            ),
        ],
        "memory_read" => vec![
            (r#"{}"#, Expect::Contains("Memory Index")),
            (
                r#"{"name":"note.md"}"#,
                Expect::Contains("named-memory-body"),
            ),
            (r#"{"name":"../escape.md"}"#, Expect::Runs), // confinement escape
        ],
        other => panic!(
            "t4 BC-3a: pure-read tool `{other}` has no behavioral input fixture — \
             a tool that cannot be exercised cannot be PureRead. Add cases to \
             `input_cases` (or remove it from PURE_READ_TOOLS)."
        ),
    }
}

#[test]
fn egress_every_pure_read_tool_has_behavioral_fixtures() {
    // Runs on every platform (the behavioral run is Linux-only, but the no-opt-out
    // invariant is checked everywhere): each whitelisted tool maps to ≥1 input.
    for &tool in bwoc_harness::policy::PURE_READ_TOOLS {
        let cases = input_cases(tool); // panics if unmapped
        assert!(
            !cases.is_empty(),
            "pure-read tool `{tool}` must have at least one behavioral input case"
        );
    }
}

// ---------------------------------------------------------------------------
// Static floor (UNIVERSAL) — forbidden effect symbols in pure-read sources
// ---------------------------------------------------------------------------

/// Effect symbols that must not appear in a pure-read tool's source: network,
/// process spawn, and filesystem mutation. A coarse, fast floor (t4 BC-4).
const FORBIDDEN_SYMBOLS: &[&str] = &[
    "reqwest",
    "std::net",
    "tokio::net",
    "TcpStream",
    "Command::new",
    "tokio::process",
    "fs::write",
    "create_dir",
    "OpenOptions",
    "fs::remove",
    "append",
];

#[test]
fn egress_static_floor_pure_read_tools_have_no_forbidden_symbols() {
    for &tool in bwoc_harness::policy::PURE_READ_TOOLS {
        let src = pure_read_tool_source(tool);
        for sym in FORBIDDEN_SYMBOLS {
            assert!(
                !src.contains(sym),
                "static floor: pure-read tool `{tool}` source references forbidden \
                 effect symbol `{sym}` — it is not side-effect-free"
            );
        }
    }
}

/// Gather the source a pure-read tool's behavior depends on, within its defining
/// file: the `impl ToolImpl` block plus every same-file top-level helper it calls
/// (transitively). Cross-file helpers and transitive deps are out of scope for the
/// static floor (behavioral covers them — t4 BC-4).
///
/// **No opt-out**: if the tool's impl block cannot be located, this panics (a tool
/// we cannot scan cannot be claimed PureRead).
fn pure_read_tool_source(tool: &str) -> String {
    let files = ["src/tools/impls.rs", "src/tools/extra_tools.rs"];
    for file in files {
        let src = read_src(file);
        for (_, body) in impl_tool_blocks(&src) {
            if tool_name_of_block(&body).as_deref() != Some(tool) {
                continue;
            }
            // Pull in same-file top-level helpers, transitively (e.g. grep_walk).
            let helpers = top_level_fns(&src);
            let mut included = body.clone();
            let mut seen: HashSet<String> = HashSet::new();
            let mut work = vec![body];
            while let Some(chunk) = work.pop() {
                for (name, fbody) in &helpers {
                    if seen.contains(name) {
                        continue;
                    }
                    if chunk.contains(&format!("{name}(")) {
                        seen.insert(name.clone());
                        included.push('\n');
                        included.push_str(fbody);
                        work.push(fbody.clone());
                    }
                }
            }
            return included;
        }
    }
    panic!(
        "static floor: no `impl ToolImpl` block found for pure-read tool `{tool}` \
         in {files:?} — a tool that cannot be located cannot be proven PureRead"
    );
}

// ---------------------------------------------------------------------------
// Single construction site for Capability::PureRead (t4 BC-3b)
// ---------------------------------------------------------------------------

#[test]
fn egress_pure_read_has_single_construction_site() {
    let src = read_src("src/policy/mod.rs");

    // Within `classify_capability`, PureRead must be minted exactly once and only
    // behind the whitelist membership check.
    let body = fn_body(&src, "fn classify_capability(")
        .expect("classify_capability must exist in policy/mod.rs");
    assert!(
        body.contains("PURE_READ_TOOLS.contains(&tool_name)"),
        "classify_capability must gate PureRead on PURE_READ_TOOLS membership"
    );
    let in_classifier = count_code_occurrences(&body, "Capability::PureRead");
    assert_eq!(
        in_classifier, 1,
        "classify_capability constructs Capability::PureRead {in_classifier} times — \
         expected exactly one (the membership-check arm)"
    );

    // Whole-(production)-file guard: across the module's non-test code, PureRead is
    // *constructed* exactly once (doc comments and any match-pattern arms excluded).
    // No second arm may bypass the whitelist enumeration (t4 BC-3b). Test code is
    // excluded — it legitimately names the variant in golden tables.
    let production = src.split("#[cfg(test)]").next().unwrap_or(&src);
    let constructions = count_code_occurrences(production, "Capability::PureRead");
    assert_eq!(
        constructions, 1,
        "Capability::PureRead is constructed {constructions} times across policy/mod.rs \
         (non-test code) — expected exactly one construction site. A second site would let \
         a tool become PureRead without passing the PURE_READ_TOOLS membership check."
    );
}

// ---------------------------------------------------------------------------
// Source-scanning helpers
// ---------------------------------------------------------------------------

fn read_src(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Match `{`/`}` from the first `{` at or after `from`, returning the index just
/// past the matching close brace. Tool sources contain only balanced braces
/// (incl. inside `json!` macros), so a char scan is sufficient here.
fn brace_match_end(src: &str, from: usize) -> Option<usize> {
    let open = from + src[from..].find('{')?;
    let mut depth = 0i32;
    for (idx, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + idx + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// All `impl ToolImpl for <Struct> { … }` blocks in a file: `(struct, full text)`.
fn impl_tool_blocks(src: &str) -> Vec<(String, String)> {
    const MARKER: &str = "impl ToolImpl for ";
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(rel) = src[search..].find(MARKER) {
        let start = search + rel;
        let after = start + MARKER.len();
        let name_end = src[after..]
            .find(|c: char| c.is_whitespace() || c == '{')
            .map(|i| after + i)
            .unwrap_or(src.len());
        let struct_name = src[after..name_end].trim().to_string();
        match brace_match_end(src, name_end) {
            Some(end) => {
                out.push((struct_name, src[start..end].to_string()));
                search = end;
            }
            None => break,
        }
    }
    out
}

/// The tool name a ToolImpl block declares via `fn name(&self) -> &'static str`.
fn tool_name_of_block(body: &str) -> Option<String> {
    let i = body.find("fn name(")?;
    let q1 = i + body[i..].find('"')?;
    let q2 = q1 + 1 + body[q1 + 1..].find('"')?;
    Some(body[q1 + 1..q2].to_string())
}

/// Top-level (column-0) free functions in a file: `(name, full text)`. Methods
/// inside `impl` blocks are indented and thus excluded.
fn top_level_fns(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line_start in line_starts(src) {
        let rest = &src[line_start..];
        let decl = rest
            .strip_prefix("fn ")
            .or_else(|| rest.strip_prefix("pub fn "))
            .or_else(|| rest.strip_prefix("async fn "))
            .or_else(|| rest.strip_prefix("pub async fn "))
            .or_else(|| rest.strip_prefix("pub(crate) fn "));
        let Some(decl) = decl else { continue };
        let Some(paren) = decl.find('(') else {
            continue;
        };
        let name = decl[..paren].trim().to_string();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        if let Some(end) = brace_match_end(src, line_start) {
            out.push((name, src[line_start..end].to_string()));
        }
    }
    out
}

/// Byte offsets of the start of every line (offset 0 and each char after `\n`).
fn line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, ch) in src.char_indices() {
        if ch == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Body text of the first free function whose declaration starts with `sig` at
/// column 0 (e.g. `"fn classify_capability("`).
fn fn_body(src: &str, sig: &str) -> Option<String> {
    for line_start in line_starts(src) {
        if src[line_start..].starts_with(sig) {
            let end = brace_match_end(src, line_start)?;
            return Some(src[line_start..end].to_string());
        }
    }
    None
}

/// Count occurrences of `needle` in `src` on **code** lines only (lines whose
/// trimmed start is not a `//` comment) and **not** as a match pattern
/// (`needle =>`). This isolates value *constructions* from doc comments and
/// pattern arms.
fn count_code_occurrences(src: &str, needle: &str) -> usize {
    let mut count = 0;
    for line in src.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut from = 0;
        while let Some(rel) = line[from..].find(needle) {
            let at = from + rel;
            let tail = line[at + needle.len()..].trim_start();
            if !tail.starts_with("=>") {
                count += 1;
            }
            from = at + needle.len();
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Behavioral proof (PRIMARY) — Linux only (Landlock)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux_behavioral {
    use super::{Expect, input_cases};
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};

    /// Run **one** pure-read tool through all its input cases inside a Landlock
    /// domain that denies every FS write and every TCP connect/bind. Each tool
    /// gets a fresh thread (Landlock self-restriction is per-thread and
    /// irreversible) so the main test process keeps its privileges.
    ///
    /// Order, per t4 BC-1b: arm the sandbox → *prove it is armed* → only then run
    /// the tool. Any failure to arm or self-test returns `Err`, which fails the
    /// test (never a skip-pass).
    fn run_tool_under_sandbox(
        tool: String,
        cases: Vec<(&'static str, Expect)>,
        workdir: PathBuf,
    ) -> Result<(), String> {
        let tool_label = tool.clone(); // for the panic message after `tool` moves
        std::thread::spawn(move || -> Result<(), String> {
            arm_sandbox()?;
            assert_tripwire_armed(&workdir)?;

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("runtime build failed: {e}"))?;
            let reg = bwoc_harness::tools::registry::default_registry();
            let ctx = bwoc_harness::tools::ToolContext::new(workdir.clone());

            for (args, expect) in &cases {
                let out = rt.block_on(bwoc_harness::tools::registry::dispatch(
                    &reg, &tool, args, &ctx,
                ));
                if let Expect::Contains(needle) = expect {
                    if !out.contains(needle) {
                        return Err(format!(
                            "tool `{tool}` args `{args}`: expected output to contain `{needle}`, \
                             got: {out}"
                        ));
                    }
                }
                // Reaching here means the tool completed under deny-all-write +
                // deny-all-egress without tripping the sandbox into an error it
                // could not absorb — i.e. it performed no write and no egress.
            }
            Ok(())
        })
        .join()
        .map_err(|_| format!("sandboxed thread for `{tool_label}` panicked"))?
    }

    /// Install the Landlock domain on the current thread: handle (and add no
    /// rules for) all write-family FS access and TCP connect/bind, so reads stay
    /// unrestricted while every write and every egress is denied. `HardRequirement`
    /// makes an unsupported access a hard error — fail-closed (t4 BC-1b).
    fn arm_sandbox() -> Result<(), String> {
        use landlock::{
            ABI, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus,
        };
        // ABI V4 (Linux 6.7+) is the first with network access control; the t4
        // behavioral gate requires it, hence HardRequirement.
        let abi = ABI::V4;
        let status = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_write(abi))
            .map_err(|e| format!("landlock handle fs-write: {e}"))?
            .handle_access(AccessNet::ConnectTcp | AccessNet::BindTcp)
            .map_err(|e| format!("landlock handle net: {e}"))?
            .create()
            .map_err(|e| format!("landlock create ruleset: {e}"))?
            .restrict_self()
            .map_err(|e| format!("landlock restrict_self: {e}"))?;
        if status.ruleset != RulesetStatus::FullyEnforced {
            return Err(format!(
                "landlock sandbox not fully enforced (got {:?}) — refusing to run \
                 the behavioral proof against a partial sandbox",
                status.ruleset
            ));
        }
        Ok(())
    }

    /// Empirically prove the sandbox actually denies — a no-op sandbox must never
    /// yield a false green (t4 BC-1b). Both probes must be refused with `EACCES`
    /// (`PermissionDenied`): the FS write tripwire by creating a file, the egress
    /// tripwire by a TCP connect to an IP literal (no DNS, so the syscall is a
    /// bare `connect()` that the LSM rejects before any packet).
    fn assert_tripwire_armed(workdir: &Path) -> Result<(), String> {
        let probe = workdir.join("__tripwire_write_probe__");
        match std::fs::File::create(&probe) {
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {}
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                return Err(
                    "FS-write tripwire NOT armed: created a file under a read-only sandbox".into(),
                );
            }
            Err(e) => {
                return Err(format!(
                    "FS-write tripwire gave a non-EACCES error (expected PermissionDenied): {e}"
                ));
            }
        }
        match std::net::TcpStream::connect("1.1.1.1:443") {
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {}
            Ok(_) => {
                return Err("egress tripwire NOT armed: a TCP connect succeeded".into());
            }
            Err(e) => {
                return Err(format!(
                    "egress tripwire gave a non-EACCES error (expected PermissionDenied — the \
                     sandbox may be a no-op): {e}"
                ));
            }
        }
        Ok(())
    }

    fn build_fixture(root: &Path) {
        use std::fs;
        fs::write(
            root.join("note.txt"),
            "hello from read_file\nNEEDLE_xyz here\n",
        )
        .unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/code.rs"), "fn f() { /* NEEDLE_xyz */ }\n").unwrap();
        fs::create_dir(root.join("memories")).unwrap();
        fs::write(
            root.join("memories/MEMORY.md"),
            "# Memory Index\nNEEDLE_xyz\n",
        )
        .unwrap();
        fs::write(root.join("memories/note.md"), "named-memory-body\n").unwrap();
    }

    #[test]
    fn egress_behavioral_pure_read_tools_make_no_egress() {
        let dir = tempfile::tempdir().expect("tempdir");
        build_fixture(dir.path());

        // Enumerate straight from the single source of truth — no skip-list (BC-3a).
        let tools: Vec<String> = bwoc_harness::policy::PURE_READ_TOOLS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(!tools.is_empty(), "PURE_READ_TOOLS is empty");

        let mut failures = Vec::new();
        for tool in tools {
            let cases = input_cases(&tool);
            if let Err(e) = run_tool_under_sandbox(tool.clone(), cases, dir.path().to_path_buf()) {
                failures.push(e);
            }
        }
        assert!(
            failures.is_empty(),
            "behavioral egress proof failed for {} case(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
