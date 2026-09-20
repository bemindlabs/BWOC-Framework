//! Markdown rendering for assistant turns in the chat transcript (runtime R5).
//!
//! Models answer in Markdown, and the transcript used to show the source. This
//! renders the parts that carry meaning in a terminal — headings, list bullets,
//! block quotes, fenced code, inline code, bold and italic — and leaves
//! everything else as written. It is deliberately small: a full CommonMark
//! parser is a dependency and a maintenance surface this pane does not need
//! (Mattaññutā), and anything it does not recognise still renders verbatim.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Render `text` as styled rows, each one transcript line. `base` is the style
/// the caller uses for this entry (e.g. dimmed while a turn streams); every
/// span inherits it and adds its own emphasis.
pub fn render(text: &str, base: Style, code: Style) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut fenced = false;
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim_start().starts_with("```") {
            // The fence itself is shown dimmed, with its language if given.
            fenced = !fenced;
            out.push(Line::from(Span::styled(
                line.to_string(),
                base.add_modifier(Modifier::DIM),
            )));
            continue;
        }
        if fenced {
            out.push(Line::from(Span::styled(line.to_string(), code)));
            continue;
        }
        out.push(block(line, base, code));
    }
    out
}

/// One non-fenced line: a heading, a bullet, a quote, or plain text — each with
/// its inline spans.
fn block(line: &str, base: Style, code: Style) -> Line<'static> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];

    if let Some(rest) = heading(trimmed) {
        let mut spans = vec![Span::styled(indent.to_string(), base)];
        spans.extend(inline(rest, base.add_modifier(Modifier::BOLD), code));
        return Line::from(spans);
    }
    if let Some(rest) = trimmed.strip_prefix("> ") {
        let mut spans = vec![Span::styled(
            format!("{indent}▏ "),
            base.add_modifier(Modifier::DIM),
        )];
        spans.extend(inline(rest, base.add_modifier(Modifier::ITALIC), code));
        return Line::from(spans);
    }
    if let Some((marker, rest)) = bullet(trimmed) {
        let mut spans = vec![Span::styled(
            format!("{indent}{marker} "),
            base.add_modifier(Modifier::BOLD),
        )];
        spans.extend(inline(rest, base, code));
        return Line::from(spans);
    }
    let mut spans = vec![Span::styled(indent.to_string(), base)];
    spans.extend(inline(trimmed, base, code));
    Line::from(spans)
}

/// The text after a `#`-heading marker (1–6 hashes then a space).
fn heading(trimmed: &str) -> Option<&str> {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    (1..=6)
        .contains(&hashes)
        .then(|| trimmed[hashes..].strip_prefix(' '))
        .flatten()
}

/// `(marker, rest)` for an unordered (`-`, `*`, `+`) or ordered (`1.`) item.
fn bullet(trimmed: &str) -> Option<(String, &str)> {
    for m in ['-', '*', '+'] {
        if let Some(rest) = trimmed.strip_prefix(m)
            && rest.starts_with(' ')
        {
            return Some(("•".to_string(), rest.trim_start()));
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = trimmed[digits..].strip_prefix(". ")
    {
        return Some((format!("{}.", &trimmed[..digits]), rest));
    }
    None
}

/// Inline spans: `` `code` ``, `**bold**` and `*italic*` / `_italic_`.
/// An unclosed marker is literal text, so a stray `*` never eats the rest.
fn inline(text: &str, base: Style, code: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let marker = if rest.starts_with("**") {
            Some(("**", base.add_modifier(Modifier::BOLD)))
        } else if bytes[i] == b'`' {
            Some(("`", code))
        } else if bytes[i] == b'*' {
            Some(("*", base.add_modifier(Modifier::ITALIC)))
        } else if bytes[i] == b'_' {
            Some(("_", base.add_modifier(Modifier::ITALIC)))
        } else {
            None
        };
        let Some((delim, style)) = marker else {
            let ch = rest.chars().next().expect("non-empty rest");
            plain.push(ch);
            i += ch.len_utf8();
            continue;
        };
        let after = i + delim.len();
        match text[after..].find(delim) {
            Some(rel) if rel > 0 => {
                if !plain.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut plain), base));
                }
                spans.push(Span::styled(text[after..after + rel].to_string(), style));
                i = after + rel + delim.len();
            }
            // Unclosed (or empty) marker: literal.
            _ => {
                plain.push_str(delim);
                i = after;
            }
        }
    }
    if !plain.is_empty() {
        spans.push(Span::styled(plain, base));
    }
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Style {
        Style::default()
    }
    fn code() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// The visible text of a rendered line, markers included.
    fn flat(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn one(text: &str) -> Line<'static> {
        render(text, base(), code()).remove(0)
    }

    #[test]
    fn a_heading_is_bold_without_its_hashes() {
        let line = one("## Title here");
        assert_eq!(flat(&line), "Title here");
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn hashes_without_a_space_are_not_a_heading() {
        assert_eq!(flat(&one("#nothashtag")), "#nothashtag");
        assert_eq!(flat(&one("####### seven")), "####### seven");
    }

    #[test]
    fn bullets_and_numbers_get_a_marker() {
        assert_eq!(flat(&one("- item")), "• item");
        assert_eq!(flat(&one("  * nested")), "  • nested");
        assert_eq!(flat(&one("3. third")), "3. third");
        // A bare dash is text, not a bullet.
        assert_eq!(flat(&one("-nope")), "-nope");
    }

    #[test]
    fn inline_code_and_emphasis_lose_their_markers() {
        let line = one("run `bwoc list` when **ready** or _later_");
        assert_eq!(flat(&line), "run bwoc list when ready or later");
        let styles: Vec<_> = line.spans.iter().map(|s| s.style).collect();
        assert!(
            styles
                .iter()
                .any(|s| s.add_modifier.contains(Modifier::BOLD))
        );
        assert!(
            styles
                .iter()
                .any(|s| s.add_modifier.contains(Modifier::ITALIC))
        );
        assert!(
            styles
                .iter()
                .any(|s| s.add_modifier.contains(Modifier::REVERSED))
        );
    }

    #[test]
    fn an_unclosed_marker_stays_literal() {
        assert_eq!(flat(&one("2 * 3 = 6")), "2 * 3 = 6");
        assert_eq!(flat(&one("a `backtick")), "a `backtick");
        assert_eq!(flat(&one("**bold")), "**bold");
    }

    #[test]
    fn a_fenced_block_keeps_its_content_verbatim() {
        let lines = render("```rust\nlet x = *p; // **not bold**\n```", base(), code());
        assert_eq!(flat(&lines[0]), "```rust");
        assert_eq!(flat(&lines[1]), "let x = *p; // **not bold**");
        assert_eq!(lines[1].spans[0].style, code());
        assert_eq!(flat(&lines[2]), "```");
    }

    #[test]
    fn a_quote_is_marked_and_italic() {
        let line = one("> quoted");
        assert_eq!(flat(&line), "▏ quoted");
        assert!(line.spans[1].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn plain_text_survives_unchanged() {
        let text = "ปกติ — plain text with no markup";
        assert_eq!(flat(&one(text)), text);
    }
}
