//! `/` commands and `@` file mentions for the chat input line.
//!
//! Everything here is pure (or touches only the filesystem under the session's
//! workdir) so the TUI key handling stays thin and the rules are unit-tested.
//! No protocol change: every command maps onto an existing [`ChatInput`].
//!
//! [`ChatInput`]: bwoc_core::chat_proto::ChatInput

use std::path::{Path, PathBuf};

/// The commands the chat input understands, with a one-line description each.
/// Order is the popup's order.
pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "list commands and input shortcuts"),
    (
        "/clear",
        "forget this conversation (deletes its saved history)",
    ),
    (
        "/mode",
        "show or set permission mode: default | accept_edits | bypass",
    ),
    ("/quit", "end the session"),
    ("/exit", "end the session"),
];

/// Permission modes `/mode` accepts — the ones `F2` cycles.
pub const MODES: &[&str] = &["default", "accept_edits", "bypass"];

/// A parsed `/` command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Slash {
    Help,
    Clear,
    Mode(Option<String>),
    Quit,
    Unknown(String),
}

/// Parse a submitted line as a `/` command. `None` means "send it as a message":
/// the line doesn't start with `/`, or its first word holds another `/` (a path
/// such as `/etc/hosts is empty` is prose, not a command).
pub fn parse_slash(line: &str) -> Option<Slash> {
    let line = line.trim();
    let rest = line.strip_prefix('/')?;
    let mut words = rest.split_whitespace();
    let name = words.next().unwrap_or("");
    if name.contains('/') {
        return None;
    }
    Some(match name {
        "help" | "?" => Slash::Help,
        "clear" => Slash::Clear,
        "mode" => Slash::Mode(words.next().map(str::to_string)),
        "quit" | "exit" => Slash::Quit,
        other => Slash::Unknown(other.to_string()),
    })
}

/// Commands whose name starts with what is typed after `/`. Only while the
/// command word is still being typed (no space yet); `None` otherwise.
pub fn slash_matches(input: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let typed = input.strip_prefix('/')?;
    if typed.contains(char::is_whitespace) || typed.contains('/') {
        return None;
    }
    Some(
        COMMANDS
            .iter()
            .filter(|(name, _)| name[1..].starts_with(typed))
            .copied()
            .collect(),
    )
}

/// The `@token` the cursor sits in: `(byte offset of '@', text after '@')`.
/// The `@` must start the line or follow whitespace, so `me@host` is not one.
pub fn mention_at(input: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = &input[..cursor];
    let start = before
        .rfind(char::is_whitespace)
        .map(|i| i + before[i..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(0);
    let token = &before[start..];
    let query = token.strip_prefix('@')?;
    // The whole token (up to the next space) must be the one being completed.
    Some((start, query))
}

/// Directories never listed for `@` completion.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__"];
/// Stop walking after this many files — completion stays instant in huge trees.
const MAX_FILES: usize = 5_000;

/// Every file under `root` as a `/`-separated relative path, skipping
/// [`SKIP_DIRS`] and dot-directories, capped at [`MAX_FILES`]. Sorted.
pub fn list_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_ref()) {
                    stack.push(entry.path());
                }
            } else if kind.is_file() {
                if let Ok(rel) = entry.path().strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
                if out.len() >= MAX_FILES {
                    out.sort();
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// Up to `max` files matching `query` (case-insensitive substring). Files whose
/// name starts with the query rank first, then shorter paths.
pub fn filter_files(files: &[String], query: &str, max: usize) -> Vec<String> {
    let q = query.to_lowercase();
    let mut hits: Vec<(bool, usize, &String)> = files
        .iter()
        .filter_map(|f| {
            let lower = f.to_lowercase();
            if !lower.contains(&q) {
                return None;
            }
            let base = lower.rsplit('/').next().unwrap_or(&lower);
            Some((!base.starts_with(&q), f.len(), f))
        })
        .collect();
    hits.sort();
    hits.into_iter()
        .take(max)
        .map(|(_, _, f)| f.clone())
        .collect()
}

/// Replace the token at `start..cursor` with `replacement` plus one space.
/// Returns the new line and cursor.
pub fn complete(input: &str, start: usize, cursor: usize, replacement: &str) -> (String, usize) {
    let mut line = String::with_capacity(input.len() + replacement.len() + 1);
    line.push_str(&input[..start]);
    line.push_str(replacement);
    line.push(' ');
    let new_cursor = line.len();
    line.push_str(input[cursor..].trim_start());
    (line, new_cursor)
}

/// Largest file content attached per `@` mention.
pub const MAX_ATTACH_BYTES: usize = 32 * 1024;

/// One `@` mention resolved at send time.
#[derive(Debug, PartialEq, Eq)]
pub enum Attached {
    /// Attached `path` (`bytes` read; `truncated` when it hit the cap).
    File {
        path: String,
        bytes: usize,
        truncated: bool,
    },
    /// Named a file that could not be attached, with the reason.
    Skipped { path: String, reason: &'static str },
}

/// Append the content of every `@path` in `text` that names a file under
/// `root`, Claude-Code style. Tokens that aren't files (e.g. `@busaba`) are left
/// alone. A path resolving outside `root` (via `..` or a symlink), a binary file
/// and an unreadable file are skipped and reported. Each file is attached once.
pub fn expand_mentions(text: &str, root: &Path) -> (String, Vec<Attached>) {
    let Ok(root) = root.canonicalize() else {
        return (text.to_string(), Vec::new());
    };
    let mut out = text.to_string();
    let mut report = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for word in text.split_whitespace() {
        let Some(rel) = word.strip_prefix('@') else {
            continue;
        };
        let rel = rel.trim_end_matches([',', '.', ';', ':', ')', '?', '!']);
        if rel.is_empty() {
            continue;
        }
        let candidate = root.join(rel);
        if !candidate.is_file() {
            continue;
        }
        let Ok(real) = candidate.canonicalize() else {
            continue;
        };
        if seen.contains(&real) {
            continue;
        }
        seen.push(real.clone());
        if !real.starts_with(&root) {
            report.push(Attached::Skipped {
                path: rel.to_string(),
                reason: "outside the project",
            });
            continue;
        }
        let Ok(bytes) = std::fs::read(&real) else {
            report.push(Attached::Skipped {
                path: rel.to_string(),
                reason: "unreadable",
            });
            continue;
        };
        let truncated = bytes.len() > MAX_ATTACH_BYTES;
        let mut body = &bytes[..bytes.len().min(MAX_ATTACH_BYTES)];
        if bytes.contains(&0) {
            report.push(Attached::Skipped {
                path: rel.to_string(),
                reason: "binary",
            });
            continue;
        }
        // Cut back to a char boundary if the cap split a UTF-8 sequence.
        let content = loop {
            match std::str::from_utf8(body) {
                Ok(s) => break Some(s),
                Err(e) if truncated && e.error_len().is_none() => {
                    body = &body[..e.valid_up_to()];
                }
                Err(_) => break None,
            }
        };
        let Some(content) = content else {
            report.push(Attached::Skipped {
                path: rel.to_string(),
                reason: "not UTF-8 text",
            });
            continue;
        };
        let note = if truncated {
            format!(" truncated=\"first {} bytes\"", content.len())
        } else {
            String::new()
        };
        out.push_str(&format!(
            "\n\n<file path=\"{}\"{note}>\n{content}\n</file>",
            escape_attr(rel)
        ));
        report.push(Attached::File {
            path: rel.to_string(),
            bytes: content.len(),
            truncated,
        });
    }
    (out, report)
}

/// Escape a value for a double-quoted markup attribute — a Unix filename may
/// hold `"`, `<`, `>` or `&`.
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_slash_commands_and_prose() {
        assert_eq!(parse_slash("/help"), Some(Slash::Help));
        assert_eq!(parse_slash("  /clear "), Some(Slash::Clear));
        assert_eq!(parse_slash("/mode"), Some(Slash::Mode(None)));
        assert_eq!(
            parse_slash("/mode bypass"),
            Some(Slash::Mode(Some("bypass".into())))
        );
        assert_eq!(parse_slash("/exit"), Some(Slash::Quit));
        assert_eq!(parse_slash("/nope"), Some(Slash::Unknown("nope".into())));
        assert_eq!(parse_slash("/etc/hosts is empty"), None);
        assert_eq!(parse_slash("hello /help"), None);
    }

    #[test]
    fn slash_matches_while_typing_the_name() {
        fn names(v: Vec<(&'static str, &'static str)>) -> Vec<&'static str> {
            v.into_iter().map(|c| c.0).collect()
        }
        assert_eq!(names(slash_matches("/").unwrap()).len(), COMMANDS.len());
        assert_eq!(names(slash_matches("/cl").unwrap()), ["/clear"]);
        assert_eq!(names(slash_matches("/e").unwrap()), ["/exit"]);
        assert!(slash_matches("/mode by").is_none());
        assert!(slash_matches("/etc/").is_none());
        assert!(slash_matches("hi").is_none());
    }

    #[test]
    fn mention_at_needs_a_token_start() {
        assert_eq!(mention_at("@src/ma", 7), Some((0, "src/ma")));
        assert_eq!(mention_at("look at @lib", 12), Some((8, "lib")));
        assert_eq!(mention_at("look at @", 9), Some((8, "")));
        assert_eq!(mention_at("me@host", 7), None);
        assert_eq!(mention_at("@a b", 4), None);
        assert_eq!(
            mention_at("ดู @ไฟล์", "ดู @ไฟล์".len()),
            Some(("ดู ".len(), "ไฟล์"))
        );
    }

    #[test]
    fn filter_ranks_basename_prefix_then_length() {
        let files: Vec<String> = ["docs/main.md", "src/domain.rs", "src/main.rs", "main.rs"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            filter_files(&files, "MAIN", 10),
            ["main.rs", "src/main.rs", "docs/main.md", "src/domain.rs"]
        );
        assert_eq!(filter_files(&files, "main", 1), ["main.rs"]);
        assert!(filter_files(&files, "zzz", 10).is_empty());
    }

    #[test]
    fn complete_replaces_the_token() {
        assert_eq!(complete("/cl", 0, 3, "/clear"), ("/clear ".into(), 7));
        let (line, cur) = complete("see @sr now", 4, 7, "@src/main.rs");
        assert_eq!(line, "see @src/main.rs now");
        assert_eq!(cur, "see @src/main.rs ".len());
    }

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("bwoc-tui-complete-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn list_files_skips_noise_dirs() {
        let d = tmp("list");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("target/debug")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("src/a.rs"), "").unwrap();
        std::fs::write(d.join("README.md"), "").unwrap();
        std::fs::write(d.join("target/debug/x"), "").unwrap();
        std::fs::write(d.join(".git/HEAD"), "").unwrap();
        assert_eq!(list_files(&d), ["README.md", "src/a.rs"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn expand_attaches_text_files_once_and_skips_the_rest() {
        let d = tmp("expand");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(d.join("bin.dat"), [0u8, 1, 2]).unwrap();
        let (text, report) = expand_mentions(
            "explain @src/a.rs, and @src/a.rs again; ask @busaba; @bin.dat",
            &d,
        );
        assert!(text.starts_with("explain @src/a.rs, and"));
        assert_eq!(
            text.matches("<file path=\"src/a.rs\">\nfn a() {}\n</file>")
                .count(),
            1
        );
        assert_eq!(
            report,
            [
                Attached::File {
                    path: "src/a.rs".into(),
                    bytes: 9,
                    truncated: false
                },
                Attached::Skipped {
                    path: "bin.dat".into(),
                    reason: "binary"
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn attach_escapes_the_path_attribute() {
        assert_eq!(escape_attr(r#"a"b<c>&.rs"#), "a&quot;b&lt;c&gt;&amp;.rs");
    }

    #[test]
    fn expand_refuses_paths_outside_the_root() {
        let d = tmp("outside");
        let inner = d.join("proj");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(d.join("secret.txt"), "s3cret").unwrap();
        let (text, report) = expand_mentions("read @../secret.txt", &inner);
        assert!(!text.contains("s3cret"));
        assert_eq!(
            report,
            [Attached::Skipped {
                path: "../secret.txt".into(),
                reason: "outside the project"
            }]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn expand_truncates_at_the_cap_on_a_char_boundary() {
        let d = tmp("cap");
        // 3-byte chars so the cap lands mid-sequence.
        let big = "ก".repeat(MAX_ATTACH_BYTES / 3 + 10);
        std::fs::write(d.join("big.txt"), &big).unwrap();
        let (text, report) = expand_mentions("@big.txt", &d);
        let Attached::File {
            bytes, truncated, ..
        } = &report[0]
        else {
            panic!("expected attach, got {report:?}");
        };
        assert!(*truncated);
        assert!(*bytes <= MAX_ATTACH_BYTES && bytes % 3 == 0);
        assert!(text.contains("truncated=\"first"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
