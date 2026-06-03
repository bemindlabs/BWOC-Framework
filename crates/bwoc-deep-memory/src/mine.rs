//! `mine` ingestion — turn session files under a path into memory chunks.
//!
//! Walks `<path>` (a file or a directory) for text-bearing session artifacts,
//! splits each into paragraph-bounded chunks, and yields `(source, chunk)`
//! pairs ready to embed + store. Binary and oversized files are skipped.

use std::path::{Path, PathBuf};

/// File extensions treated as text we can mine.
const TEXT_EXTS: &[&str] = &["md", "txt", "jsonl", "json", "log"];

/// Skip any single file larger than this (5 MiB) — Mattaññutā, don't try to
/// embed a runaway log.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Target upper bound for a chunk, in characters. Paragraphs are accumulated up
/// to this size; a single paragraph longer than this is hard-split.
const CHUNK_CHARS: usize = 1200;

/// A mined chunk: where it came from and the text itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Source label — the file path relative to the mined root (or its name).
    pub source: String,
    /// The chunk text.
    pub text: String,
}

/// Collect mineable chunks from `root` (a file or directory).
pub fn collect(root: &Path) -> std::io::Result<Vec<Chunk>> {
    let mut files = Vec::new();
    gather_files(root, &mut files)?;
    files.sort();

    let mut chunks = Vec::new();
    for file in &files {
        let Ok(meta) = std::fs::metadata(file) else {
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(file) else {
            continue;
        };
        let label = source_label(root, file);
        for text in chunk_text(&body) {
            chunks.push(Chunk {
                source: label.clone(),
                text,
            });
        }
    }
    Ok(chunks)
}

/// Recursively gather text files under `path` into `out`.
fn gather_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let meta = std::fs::metadata(path)?;
    if meta.is_file() {
        if is_text_file(path) {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if meta.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            gather_files(&entry.path(), out)?;
        }
    }
    Ok(())
}

/// Whether `path` has a mineable text extension.
fn is_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TEXT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Path label relative to the mined root; falls back to the file name.
fn source_label(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            file.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.to_string_lossy().into_owned())
        })
}

/// Split `body` into chunks bounded by `CHUNK_CHARS`, preferring blank-line
/// (paragraph) boundaries. Blank-only fragments are dropped.
pub fn chunk_text(body: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();

    for para in body.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        // A paragraph that alone exceeds the cap is hard-split.
        if para.chars().count() > CHUNK_CHARS {
            flush(&mut cur, &mut chunks);
            for piece in hard_split(para, CHUNK_CHARS) {
                chunks.push(piece);
            }
            continue;
        }
        if cur.chars().count() + para.chars().count() > CHUNK_CHARS {
            flush(&mut cur, &mut chunks);
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(para);
    }
    flush(&mut cur, &mut chunks);
    chunks
}

fn flush(cur: &mut String, chunks: &mut Vec<String>) {
    let t = cur.trim();
    if !t.is_empty() {
        chunks.push(t.to_string());
    }
    cur.clear();
}

/// Hard-split an over-long paragraph into ≤ `cap`-char pieces on char
/// boundaries (so multibyte text never splits mid-codepoint).
fn hard_split(s: &str, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut count = 0; // track char count incrementally — avoid O(n²) re-counting.
    for ch in s.chars() {
        if count == cap {
            out.push(std::mem::take(&mut buf));
            count = 0;
        }
        buf.push(ch);
        count += 1;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_split_on_paragraphs() {
        let body = "Para one.\n\nPara two.\n\nPara three.";
        let chunks = chunk_text(body);
        // All three are short → coalesced into a single chunk.
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("Para one."));
        assert!(chunks[0].contains("Para three."));
    }

    #[test]
    fn blank_fragments_dropped() {
        assert!(chunk_text("\n\n   \n\n").is_empty());
    }

    #[test]
    fn long_paragraph_hard_split() {
        let big = "x".repeat(CHUNK_CHARS * 2 + 10);
        let chunks = chunk_text(&big);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHARS));
    }

    #[test]
    fn coalesces_until_cap() {
        // Two paragraphs that together exceed the cap → two chunks.
        let a = "a".repeat(CHUNK_CHARS - 50);
        let b = "b".repeat(100);
        let chunks = chunk_text(&format!("{a}\n\n{b}"));
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn hard_split_respects_char_boundaries() {
        let s = "é".repeat(CHUNK_CHARS + 5); // 2-byte chars
        for piece in hard_split(&s, CHUNK_CHARS) {
            assert!(piece.chars().count() <= CHUNK_CHARS);
        }
    }

    #[test]
    fn collect_reads_dir_recursively() {
        let dir = std::env::temp_dir().join(format!("bwoc-mine-test-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("a.md"), "hello from a").unwrap();
        std::fs::write(sub.join("b.txt"), "hello from b").unwrap();
        std::fs::write(dir.join("skip.png"), [0u8, 1, 2]).unwrap();

        let chunks = collect(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().any(|c| c.source == "a.md"));
        assert!(
            chunks
                .iter()
                .any(|c| c.source.replace('\\', "/") == "sub/b.txt")
        );
    }
}
