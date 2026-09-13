//! Argv normalization for the guardrail first-token checks (#483).
//!
//! The destruction (`Pāṇātipāta`) and privilege-escalation (`Bhava-taṇhā`)
//! guardrails resolve "the binary" as the first token that isn't a `VAR=val`
//! assignment. That misses every command hidden one layer down behind a
//! transparent wrapper:
//!
//! ```text
//! env rm -rf /        command rm -rf ~     timeout 5 rm -rf /
//! xargs rm -rf        sh -c 'rm -rf /'
//! ```
//!
//! all execute a destructive `rm`, yet the naive check sees `env` / `command` /
//! `timeout` / `xargs` / `sh` and waves them through. Guardrails is the one
//! layer documented as *unoverridable* by permission config, so a miss there is
//! **silent under-blocking**, not a degraded prompt.
//!
//! [`peel`] reduces a single shell segment to the argv that will actually run,
//! stripping transparent wrappers and unwrapping one `sh -c '<literal>'`. Its
//! load-bearing property is **fail-closed**: if it cannot see what a command
//! invokes (an unmodelled wrapper flag, a stdin-fed runner, a non-literal `-c`,
//! unbalanced quotes, depth exhaustion), it returns [`Peel::FailClosed`] and the
//! caller must block. *If you cannot see what the command invokes, do not
//! certify it.*
//!
//! This is deliberately **not** a POSIX shell parser (no tree-sitter, no deps):
//! it models the common wrapper forms and fails closed on everything else,
//! trading a rare over-block for never silently under-blocking. The same
//! normalizer is wired into `check_destruction`, `check_privilege_escalation`,
//! and `sandbox::scan_args` so the three cannot drift.
//!
//! **Scope of the contract.** `peel` reasons about the command *string* the
//! model emitted. It does **not** read a script *file* (`sh build.sh`) or a
//! *stdin* stream (`… | sh`): guardrails are pure/no-I/O by design, so those
//! contents are simply not visible here, and failing closed on every
//! script-file invocation would block ordinary `bash build.sh` for no gain. For
//! those, `peel` resolves the shell itself as the effective binary (the caller
//! sees `sh`, blocks nothing extra) and the **OS sandbox** — the worktree path
//! allowlist plus Landlock/seccomp in `sandbox.rs` — is the backstop that
//! confines what the script can actually do. What `peel` *can* see in the
//! string (wrappers, a `-c` literal) it certifies or fails closed on.

/// The result of reducing a shell segment to its effective argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Peel {
    /// The command that will actually run, wrappers stripped. `argv[0]` is the
    /// real binary (leading `VAR=val` assignments already removed). May be empty
    /// (nothing runs — e.g. a bare assignment or an empty segment).
    Ready(Vec<String>),
    /// The effective argv could not be determined — the caller must fail closed
    /// (treat as a potential violation and block).
    FailClosed,
}

/// Transparent wrappers whose *tail* (after the wrapper's own options) is the
/// real command. Order-independent set.
const WRAPPERS: &[&str] = &[
    "env", "command", "builtin", "exec", "nohup", "stdbuf", "setsid", "nice", "ionice", "timeout",
];

/// Shells that accept `-c '<script>'`; we unwrap one static literal.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash"];

/// Runners whose operands arrive on **stdin**, so the argv we can see never
/// names what they actually act on (`find … | xargs rm -rf`). We cannot certify
/// them — always fail closed.
const OPAQUE_RUNNERS: &[&str] = &["xargs"];

/// Cap on wrapper-nesting recursion (`timeout 5 nohup env … cmd`). Beyond this
/// we fail closed rather than chase an adversarially deep chain.
const MAX_DEPTH: usize = 4;

/// Reduce one shell segment (already split on `; && || |`) to the argv that will
/// run. See the module docs for the fail-closed contract.
pub(crate) fn peel(segment: &str) -> Peel {
    match tokenize(segment) {
        Some(tokens) => {
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            peel_tokens(&refs, 0)
        }
        // Unbalanced quotes — we cannot tokenize it, so we cannot see it.
        None => Peel::FailClosed,
    }
}

/// The basename of a path-qualified binary (`/usr/bin/rm` → `rm`), so a wrapper
/// invoked by absolute path is still recognised.
fn basename(tok: &str) -> &str {
    tok.rsplit('/').next().unwrap_or(tok)
}

/// True for a leading environment assignment (`FOO=bar`) — a valid shell prefix
/// before any command. The name part must be a plausible identifier so a flag
/// like `--opt=val` (starts with `-`) is not mistaken for one.
fn is_env_assignment(tok: &str) -> bool {
    match tok.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && name.chars().next().is_some_and(|c| !c.is_ascii_digit())
        }
        None => false,
    }
}

/// Does this de-quoted token carry a shell expansion the static view can't
/// resolve (`$VAR`, `$(...)`, backticks)? Used to reject a non-literal `-c`
/// operand — if the command itself is behind an expansion we can't see it.
fn has_expansion(tok: &str) -> bool {
    tok.contains('$') || tok.contains('`')
}

fn peel_tokens(words: &[&str], depth: usize) -> Peel {
    if depth > MAX_DEPTH {
        return Peel::FailClosed;
    }

    // Strip leading `VAR=val` assignments (valid before any command).
    let mut i = 0;
    while i < words.len() && is_env_assignment(words[i]) {
        i += 1;
    }
    let words = &words[i..];
    let Some(&first) = words.first() else {
        return Peel::Ready(Vec::new());
    };

    let head = basename(first);

    if OPAQUE_RUNNERS.contains(&head) {
        return Peel::FailClosed;
    }
    if SHELLS.contains(&head) {
        return peel_shell(words, depth);
    }
    if WRAPPERS.contains(&head) {
        return match wrapper_tail(head, words) {
            Some(tail) => peel_tokens(tail, depth + 1),
            None => Peel::FailClosed,
        };
    }

    // Not a wrapper — this is the real command.
    Peel::Ready(words.iter().map(|s| s.to_string()).collect())
}

/// Unwrap `sh -c '<script>' [name [arg…]]`. Recurses once into a static literal
/// script; fails closed on a non-literal operand (a variable/substitution) so a
/// hidden binary is never certified.
fn peel_shell(words: &[&str], depth: usize) -> Peel {
    // The effective-binary-is-the-shell fallback, used whenever this isn't a
    // recognisable `<shell> -c <literal>` (a script file, an interactive shell):
    // nothing destructive is statically visible in the shell token itself.
    // A script-file or interactive/stdin shell: its *contents* are out of a
    // pure/no-I/O guardrail's reach (see the module docs). Resolve the shell
    // itself as the effective binary — this adds no new bypass over the prior
    // behaviour, and the OS sandbox (path allowlist + Landlock/seccomp) confines
    // what the script does. We only *unwrap* the one form we can read: `-c
    // '<literal>'`.
    let as_shell = || Peel::Ready(words.iter().map(|s| s.to_string()).collect());

    match words.get(1) {
        // The only form we unwrap: `<shell> -c <script> …`.
        Some(&"-c") => {}
        // A script-file or other non-flag operand (`sh script.sh`) — not `-c`.
        Some(w) if !w.starts_with('-') => return as_shell(),
        // A bare `sh` (interactive / stdin).
        None => return as_shell(),
        // A flag combo we don't model (`sh -e -c …`, `sh -ec …`) → fail closed
        // rather than guess where the script is.
        Some(_) => return Peel::FailClosed,
    }

    let Some(&script) = words.get(2) else {
        // `sh -c` with no operand — nothing runs.
        return Peel::Ready(Vec::new());
    };

    if has_expansion(script) {
        // `sh -c "$CMD"` — the command is behind an expansion we can't resolve.
        return Peel::FailClosed;
    }

    // Recurse into the literal script.
    match tokenize(script) {
        Some(inner) => {
            let refs: Vec<&str> = inner.iter().map(String::as_str).collect();
            peel_tokens(&refs, depth + 1)
        }
        None => Peel::FailClosed,
    }
}

/// Return the sub-command slice after a wrapper's own options, or `None` to fail
/// closed on any flag we don't model (an unmodelled flag can change *what* runs,
/// e.g. `env -S`, `env -C`, `timeout --foreground`).
fn wrapper_tail<'a>(head: &str, words: &'a [&'a str]) -> Option<&'a [&'a str]> {
    // Everything after the wrapper token itself.
    let rest = &words[1..];
    match head {
        // `nohup CMD`, `builtin CMD` — no options of their own.
        "nohup" | "builtin" => nonflag_tail(rest),

        // `command [-p] CMD` — `-v`/`-V` only print (don't exec); treat as
        // unmodelled → fail closed. `-p` is safe.
        "command" => opt_tail(rest, &["-p"], &[]),

        // `exec [-c] [-l] [-a NAME] CMD`.
        "exec" => opt_tail(rest, &["-c", "-l"], &["-a"]),

        // `setsid [-c] [-f] [-w] CMD`.
        "setsid" => opt_tail(rest, &["-c", "-f", "-w", "--ctty", "--fork", "--wait"], &[]),

        // `nice [-n ADJ] CMD` / `nice --adjustment ADJ CMD`. A deprecated
        // `nice -5` (bare number) is ambiguous → fail closed.
        "nice" => opt_tail(rest, &[], &["-n", "--adjustment"]),

        // `ionice [-c CLASS] [-n LEVEL] [-t] CMD`.
        "ionice" => opt_tail(
            rest,
            &["-t", "--ignore"],
            &["-c", "-n", "--class", "--classdata"],
        ),

        // `stdbuf {-i|-o|-e} MODE… CMD` — always takes at least one io option.
        "stdbuf" => opt_tail(
            rest,
            &[],
            &["-i", "-o", "-e", "--input", "--output", "--error"],
        ),

        // `env [-i] [-u NAME]… [NAME=VAL]… CMD`. Leading assignments are already
        // stripped by peel_tokens on recursion; here we skip env's own options.
        "env" => env_tail(rest),

        // `timeout [OPTIONS] DURATION CMD`. Any leading flag (`-s`, `-k`,
        // `--foreground`, …) we don't model → fail closed; then one duration
        // token, then the command.
        "timeout" => timeout_tail(rest),

        _ => None,
    }
}

/// A wrapper with no options: the tail is the command, and it must not itself
/// start with an (unexpected) flag.
fn nonflag_tail<'a>(rest: &'a [&'a str]) -> Option<&'a [&'a str]> {
    match rest.first() {
        Some(w) if w.starts_with('-') && *w != "--" => None,
        Some(_) => Some(strip_dd(rest)),
        None => Some(rest),
    }
}

/// Skip a wrapper's options: boolean flags in `flags`, arg-taking flags in
/// `takes_arg` (consume the following token, or an `=`-attached value). Stop at
/// the first non-flag token (the command) or `--`. Any other flag → `None`.
fn opt_tail<'a>(rest: &'a [&'a str], flags: &[&str], takes_arg: &[&str]) -> Option<&'a [&'a str]> {
    let mut i = 0;
    while i < rest.len() {
        let w = rest[i];
        if w == "--" {
            return Some(&rest[i + 1..]);
        }
        if !w.starts_with('-') {
            return Some(&rest[i..]); // the command begins here
        }
        // `--flag=val` attached form for an arg-taking flag.
        if let Some((name, _)) = w.split_once('=') {
            if takes_arg.contains(&name) {
                i += 1;
                continue;
            }
        }
        if flags.contains(&w) {
            i += 1;
            continue;
        }
        if takes_arg.contains(&w) {
            i += 2; // flag + its value in the next token
            continue;
        }
        // Short arg-taking flag with an attached value (`-oL` for `-o L`).
        if takes_arg
            .iter()
            .any(|ta| ta.len() == 2 && ta.starts_with('-') && w.len() > 2 && w.starts_with(ta))
        {
            i += 1;
            continue;
        }
        return None; // unmodelled flag → fail closed
    }
    Some(&rest[rest.len()..]) // only options, no command → empty tail
}

/// `--` terminator strip for a no-option wrapper.
fn strip_dd<'a>(rest: &'a [&'a str]) -> &'a [&'a str] {
    match rest.first() {
        Some(&"--") => &rest[1..],
        _ => rest,
    }
}

/// `env`'s option grammar: assignments, `-i`/`--ignore-environment`, `-`,
/// `-u NAME`/`--unset NAME`/`--unset=NAME`, `--`. Anything else (`-C`, `-S`,
/// `-0`, unknown) → fail closed.
fn env_tail<'a>(rest: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let mut i = 0;
    while i < rest.len() {
        let w = rest[i];
        if w == "--" {
            return Some(&rest[i + 1..]);
        }
        if is_env_assignment(w) {
            i += 1;
            continue;
        }
        if !w.starts_with('-') {
            return Some(&rest[i..]);
        }
        match w {
            "-" | "-i" | "--ignore-environment" => {
                i += 1;
            }
            "-u" | "--unset" => {
                i += 2; // consume NAME
            }
            _ if w.starts_with("--unset=") => {
                i += 1;
            }
            _ => return None, // -C, -S, -0, unknown → fail closed
        }
    }
    Some(&rest[rest.len()..])
}

/// `timeout [flag…] DURATION CMD`. We don't model timeout's flags (`-s SIG`,
/// `-k DUR`, `--foreground`, …), so any leading flag fails closed; then exactly
/// one duration token, then the command.
fn timeout_tail<'a>(rest: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let first = rest.first()?;
    if first.starts_with('-') {
        return None; // an unmodelled option (e.g. --foreground) → fail closed
    }
    // `first` is the DURATION; the command is everything after it.
    Some(&rest[1..])
}

/// Minimal quote-aware tokenizer. Splits on unquoted whitespace; `'…'` is a
/// literal run (no escapes), `"…"` allows `\"` and `\\`. Quote characters are
/// stripped from the token. Returns `None` on an unterminated quote (we cannot
/// know where the token ends → the caller fails closed).
///
/// Not a full shell lexer: it does not expand anything (that's the point — an
/// expansion survives in the token text and [`has_expansion`] rejects it where
/// it matters). It exists so `sh -c 'rm -rf /'` tokenizes to three tokens, not
/// five, closing the quote-unaware gap the old `split_whitespace` had.
fn tokenize(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_tok = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_tok {
                    out.push(std::mem::take(&mut cur));
                    in_tok = false;
                }
            }
            '\'' => {
                in_tok = true;
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == '\'' {
                        closed = true;
                        break;
                    }
                    cur.push(c2);
                }
                if !closed {
                    return None; // unterminated single quote
                }
            }
            '"' => {
                in_tok = true;
                let mut closed = false;
                while let Some(c2) = chars.next() {
                    match c2 {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            // Only `\"` and `\\` are special inside "…"; keep
                            // the escaped char, drop the backslash. Other
                            // backslashes are literal.
                            match chars.peek() {
                                Some('"') | Some('\\') => {
                                    cur.push(chars.next().unwrap());
                                }
                                _ => cur.push('\\'),
                            }
                        }
                        _ => cur.push(c2),
                    }
                }
                if !closed {
                    return None; // unterminated double quote
                }
            }
            _ => {
                in_tok = true;
                cur.push(c);
            }
        }
    }
    if in_tok {
        out.push(cur);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(segment: &str) -> Vec<String> {
        match peel(segment) {
            Peel::Ready(v) => v,
            Peel::FailClosed => panic!("expected Ready for {segment:?}, got FailClosed"),
        }
    }

    fn is_fail_closed(segment: &str) -> bool {
        matches!(peel(segment), Peel::FailClosed)
    }

    #[test]
    fn plain_command_passes_through() {
        assert_eq!(ready("cargo test"), vec!["cargo", "test"]);
        assert_eq!(ready("rm -rf /tmp/x"), vec!["rm", "-rf", "/tmp/x"]);
    }

    #[test]
    fn leading_assignments_stripped() {
        assert_eq!(ready("FOO=bar rm -rf /"), vec!["rm", "-rf", "/"]);
        assert_eq!(ready("A=1 B=2 rm x"), vec!["rm", "x"]);
    }

    // ── The five documented bypass strings (#483) ────────────────────────────

    #[test]
    fn peels_env_wrapper() {
        assert_eq!(ready("env rm -rf /"), vec!["rm", "-rf", "/"]);
        assert_eq!(ready("env FOO=bar rm -rf /"), vec!["rm", "-rf", "/"]);
    }

    #[test]
    fn peels_command_wrapper() {
        assert_eq!(ready("command rm -rf ~"), vec!["rm", "-rf", "~"]);
    }

    #[test]
    fn peels_timeout_wrapper() {
        assert_eq!(ready("timeout 5 rm -rf /"), vec!["rm", "-rf", "/"]);
        assert_eq!(ready("timeout 5s rm -rf /"), vec!["rm", "-rf", "/"]);
    }

    #[test]
    fn xargs_fails_closed() {
        // Operands arrive on stdin — we can't see them, so never certify.
        assert!(is_fail_closed("xargs rm -rf"));
    }

    #[test]
    fn unwraps_sh_c_literal() {
        assert_eq!(ready("sh -c 'rm -rf /'"), vec!["rm", "-rf", "/"]);
        assert_eq!(ready("bash -c 'rm -rf /'"), vec!["rm", "-rf", "/"]);
    }

    // ── Fail-closed on the things we can't see ───────────────────────────────

    #[test]
    fn unmodelled_wrapper_flags_fail_closed() {
        assert!(is_fail_closed("env -C /etc rm -rf /")); // -C changes cwd
        assert!(is_fail_closed("env -S 'rm -rf /'")); // -S re-splits into argv
        assert!(is_fail_closed("timeout --foreground 5 rm -rf /"));
    }

    #[test]
    fn non_literal_sh_c_fails_closed() {
        assert!(is_fail_closed("sh -c \"$CMD\""));
        assert!(is_fail_closed("sh -c 'rm -rf $HOME'"));
        assert!(is_fail_closed("bash -c \"$(get_cmd)\""));
    }

    #[test]
    fn unbalanced_quotes_fail_closed() {
        assert!(is_fail_closed("sh -c 'rm -rf /"));
    }

    #[test]
    fn nested_wrappers_peel() {
        assert_eq!(ready("timeout 5 nohup rm -rf /"), vec!["rm", "-rf", "/"]);
        assert_eq!(ready("env nice -n 10 rm -rf /"), vec!["rm", "-rf", "/"]);
    }

    #[test]
    fn deep_chain_fails_closed() {
        // Beyond MAX_DEPTH nesting of wrappers → fail closed.
        assert!(is_fail_closed("env env env env env env rm -rf /"));
    }

    #[test]
    fn nice_and_stdbuf_argful_flags() {
        assert_eq!(ready("nice -n 19 rm x"), vec!["rm", "x"]);
        assert_eq!(ready("stdbuf -oL rm x"), vec!["rm", "x"]);
        assert_eq!(ready("stdbuf -o L rm x"), vec!["rm", "x"]);
        // Deprecated bare-number nice adjustment is ambiguous → fail closed.
        assert!(is_fail_closed("nice -19 rm x"));
    }

    #[test]
    fn wrapper_by_absolute_path_recognised() {
        assert_eq!(ready("/usr/bin/env rm -rf /"), vec!["rm", "-rf", "/"]);
    }

    #[test]
    fn tokenizer_handles_quotes() {
        assert_eq!(tokenize("a 'b c' d").unwrap(), vec!["a", "b c", "d"]);
        assert_eq!(tokenize("a \"b c\" d").unwrap(), vec!["a", "b c", "d"]);
        assert_eq!(tokenize("x \"a\\\"b\"").unwrap(), vec!["x", "a\"b"]);
        assert!(tokenize("a 'unterminated").is_none());
    }
}
