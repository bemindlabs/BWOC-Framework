//! Unified diffs for the chat session's [`ChatEvent::Diff`] (runtime R4c).
//!
//! Display-only: the model already sees the tool result, so this exists so a
//! frontend can show *what changed on disk* after a file-mutating tool. Kept
//! dependency-free (the dep quarantine) and bounded — a diff is capped in both
//! the work it does and the bytes it emits, because a tool may rewrite a very
//! large file and the session must not stall or flood the frontend.
//!
//! [`ChatEvent::Diff`]: bwoc_core::chat_proto::ChatEvent::Diff

/// Lines of unchanged context kept around each change.
const CONTEXT: usize = 3;
/// Largest diff body emitted, in bytes. Longer diffs are cut at a line boundary
/// and reported as truncated.
pub const MAX_DIFF_BYTES: usize = 8 * 1024;
/// Largest changed region (in lines, per side) compared line-by-line. A bigger
/// rewrite gets one coarse hunk instead — an honest summary beats a slow or
/// enormous diff.
const MAX_LCS_LINES: usize = 1_000;

/// A rendered diff plus whether it was cut short.
#[derive(Debug, PartialEq, Eq)]
pub struct Diff {
    pub text: String,
    pub truncated: bool,
}

/// Unified diff of `before` → `after`. Returns `None` when the contents match
/// (a tool that rewrote identical bytes produces no event) or when either side
/// is not UTF-8 text.
pub fn unified(before: &[u8], after: &[u8]) -> Option<Diff> {
    if before == after {
        return None;
    }
    let before = std::str::from_utf8(before).ok()?;
    let after = std::str::from_utf8(after).ok()?;
    let a: Vec<&str> = split_lines(before);
    let b: Vec<&str> = split_lines(after);

    // Trim the matching head and tail: the interesting region is between them.
    let head = a
        .iter()
        .zip(&b)
        .take_while(|(x, y)| x == y)
        .count()
        .min(a.len().min(b.len()));
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let a_mid = &a[head..a.len() - tail];
    let b_mid = &b[head..b.len() - tail];

    let mut out = String::new();
    let ctx_start = head.saturating_sub(CONTEXT);
    let ctx_end_len = tail.min(CONTEXT);
    // Hunk header counts cover the context plus the changed region.
    let a_len = (head - ctx_start) + a_mid.len() + ctx_end_len;
    let b_len = (head - ctx_start) + b_mid.len() + ctx_end_len;
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        ctx_start + 1,
        a_len,
        ctx_start + 1,
        b_len
    ));
    for line in &a[ctx_start..head] {
        out.push_str(&format!(" {line}\n"));
    }
    if a_mid.len() > MAX_LCS_LINES || b_mid.len() > MAX_LCS_LINES {
        // Too big to align line-by-line: say so plainly rather than guess.
        out.push_str(&format!(
            "-<{} lines replaced>\n+<{} lines>\n",
            a_mid.len(),
            b_mid.len()
        ));
    } else {
        for (tag, line) in align(a_mid, b_mid) {
            out.push(tag);
            out.push_str(line);
            out.push('\n');
        }
    }
    for line in &a[a.len() - tail..a.len() - tail + ctx_end_len] {
        out.push_str(&format!(" {line}\n"));
    }
    Some(cap(out))
}

/// Split into lines without a trailing empty element for a final newline.
fn split_lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

/// Line tags (`-` removed, `+` added, ` ` kept) for the changed region, via an
/// LCS table. Bounded by [`MAX_LCS_LINES`] on the caller's side.
fn align<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(char, &'a str)> {
    let (n, m) = (a.len(), b.len());
    // lcs[i][j] = length of the longest common subsequence of a[i..], b[j..].
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(('-', a[i]));
            i += 1;
        } else {
            out.push(('+', b[j]));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| ('-', *l)));
    out.extend(b[j..].iter().map(|l| ('+', *l)));
    out
}

/// Cut the body at [`MAX_DIFF_BYTES`], on a line boundary.
fn cap(text: String) -> Diff {
    if text.len() <= MAX_DIFF_BYTES {
        return Diff {
            text,
            truncated: false,
        };
    }
    // Search the BYTES for the last newline inside the cap: slicing the `str`
    // there would panic when the cut lands inside a multi-byte character. A
    // newline index is always a char boundary.
    let cut = text.as_bytes()[..MAX_DIFF_BYTES]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    Diff {
        text: text[..cut].to_string(),
        truncated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(before: &str, after: &str) -> String {
        unified(before.as_bytes(), after.as_bytes()).unwrap().text
    }

    #[test]
    fn identical_content_has_no_diff() {
        assert_eq!(unified(b"same\n", b"same\n"), None);
    }

    #[test]
    fn binary_content_has_no_diff() {
        assert_eq!(unified(&[0xff, 0xfe], b"text"), None);
    }

    #[test]
    fn one_changed_line_keeps_context() {
        let d = text("a\nb\nc\nd\ne\nf\ng\nh\n", "a\nb\nc\nD\ne\nf\ng\nh\n");
        assert_eq!(d, "@@ -1,7 +1,7 @@\n a\n b\n c\n-d\n+D\n e\n f\n g\n");
    }

    #[test]
    fn an_added_line_is_marked_and_a_removed_line_is_marked() {
        assert_eq!(text("a\nb\n", "a\nx\nb\n"), "@@ -1,2 +1,3 @@\n a\n+x\n b\n");
        assert_eq!(text("a\nx\nb\n", "a\nb\n"), "@@ -1,3 +1,2 @@\n a\n-x\n b\n");
    }

    #[test]
    fn a_new_file_is_all_additions() {
        assert_eq!(text("", "one\ntwo\n"), "@@ -1,0 +1,2 @@\n+one\n+two\n");
    }

    #[test]
    fn a_huge_rewrite_is_summarized_not_aligned() {
        let before = "old\n".repeat(MAX_LCS_LINES + 1);
        let after = "new\n".repeat(3);
        let d = text(&before, &after);
        assert!(
            d.contains(&format!("-<{} lines replaced>", MAX_LCS_LINES + 1)),
            "{d}"
        );
        assert!(d.contains("+<3 lines>"), "{d}");
    }

    #[test]
    fn a_long_non_ascii_diff_is_cut_without_panicking() {
        // The byte cap lands inside a 3-byte character on at least one line.
        let before = (0..900)
            .map(|i| format!("บรรทัด {i} ................\n"))
            .collect::<String>();
        let after = (0..900)
            .map(|i| format!("บรรทัด {i} ++++++++++++++++\n"))
            .collect::<String>();
        let d = unified(before.as_bytes(), after.as_bytes()).unwrap();
        assert!(d.truncated);
        assert!(d.text.ends_with('\n'));
        assert!(d.text.len() <= MAX_DIFF_BYTES);
    }

    #[test]
    fn a_long_diff_is_cut_on_a_line_boundary_and_flagged() {
        // Under the LCS cap (so lines really are aligned), but far over the
        // byte cap.
        let before = (0..900)
            .map(|i| format!("line {i} ........................\n"))
            .collect::<String>();
        let after = (0..900)
            .map(|i| format!("line {i} ++++++++++++++++++++++++\n"))
            .collect::<String>();
        let d = unified(before.as_bytes(), after.as_bytes()).unwrap();
        assert!(d.truncated);
        assert!(d.text.len() <= MAX_DIFF_BYTES);
        assert!(d.text.ends_with('\n'));
    }
}
