//! Markdown rendering for assistant turns in the chat transcript (runtime R5).
//!
//! Models answer in Markdown, and the transcript used to show the source. This
//! parses it as CommonMark with the GitHub extensions models lean on (tables,
//! task lists, strikethrough) via `pulldown-cmark`, and maps the result onto
//! terminal styles: headings and `**strong**` bold, `*emphasis*` italic, code
//! in the code style, quotes behind a bar, lists with markers, tables as
//! aligned columns, links as their text with the URL after it.
//!
//! A hand-written line scanner came first and read every `_` and `*` as
//! emphasis, so `snake_case` lost its underscores and `2 * 3` its stars, and
//! links, tables, escapes and nesting stayed raw. A real parser is the smaller
//! thing to keep correct than a growing list of those rules.
//!
//! Line breaks are kept as the model wrote them (a soft break is a new row,
//! not a joined paragraph), and a blank line in the source between two blocks
//! stays a blank row — the terminal is not a page to reflow.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// Render `text` as styled rows, each one transcript line. `base` is the style
/// the caller uses for this entry (e.g. dimmed while a turn streams); every
/// span inherits it and adds its own emphasis. `code` styles inline code and
/// code blocks.
pub fn render(text: &str, base: Style, code: Style) -> Vec<Line<'static>> {
    let mut r = Renderer::new(text, base, code);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        r.event(event, range);
    }
    r.finish()
}

/// One open list: the next number for an ordered list, `None` for bullets.
struct List {
    next: Option<u64>,
}

/// A table being collected: rows of cells, each cell its spans. Columns are
/// sized once the whole table is known.
#[derive(Default)]
struct Table {
    rows: Vec<Vec<Vec<Span<'static>>>>,
    header_rows: usize,
}

struct Renderer<'a> {
    source: &'a str,
    base: Style,
    code: Style,
    out: Vec<Line<'static>>,
    line: Vec<Span<'static>>,
    started: bool,
    styles: Vec<Style>,
    quote_depth: usize,
    lists: Vec<List>,
    /// Width of each open item's marker plus its space: continuation rows of
    /// an item line up under its text.
    indents: Vec<usize>,
    /// The marker the next row of the innermost item starts with.
    marker: Option<String>,
    code_block: bool,
    links: Vec<String>,
    table: Option<Table>,
    cell: Vec<Span<'static>>,
    /// Nesting of block containers, so spacing is decided at the top level.
    depth: usize,
    last_block_end: Option<usize>,
}

impl<'a> Renderer<'a> {
    fn new(source: &'a str, base: Style, code: Style) -> Self {
        Self {
            source,
            base,
            code,
            out: Vec::new(),
            line: Vec::new(),
            started: false,
            styles: vec![base],
            quote_depth: 0,
            lists: Vec::new(),
            indents: Vec::new(),
            marker: None,
            code_block: false,
            links: Vec::new(),
            table: None,
            cell: Vec::new(),
            depth: 0,
            last_block_end: None,
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&self.base)
    }

    fn push_style(&mut self, add: Modifier) {
        let s = self.style().add_modifier(add);
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    /// Open a row: quote bars, then the item marker or the indent that lines a
    /// continuation up under the item's text.
    fn start_line(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let dim = self.base.add_modifier(Modifier::DIM);
        for _ in 0..self.quote_depth {
            self.line.push(Span::styled("▏ ", dim));
        }
        let outer: usize = match self.marker {
            Some(_) => self.indents.iter().rev().skip(1).sum(),
            None => self.indents.iter().sum(),
        };
        if outer > 0 {
            self.line.push(Span::styled(" ".repeat(outer), self.base));
        }
        if let Some(marker) = self.marker.take() {
            self.line.push(Span::styled(
                format!("{marker} "),
                self.base.add_modifier(Modifier::BOLD),
            ));
        }
    }

    fn end_line(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        self.out.push(Line::from(std::mem::take(&mut self.line)));
    }

    fn text(&mut self, text: &str, style: Style) {
        if self.table.is_some() {
            self.cell.push(Span::styled(text.to_string(), style));
            return;
        }
        self.start_line();
        self.line.push(Span::styled(text.to_string(), style));
    }

    /// A top-level block starts at `start`: keep a blank row the source had
    /// between it and the block before.
    fn block_start(&mut self, start: usize) {
        if self.depth == 0 {
            if let Some(end) = self.last_block_end {
                // A block's range may already include its own closing newline.
                let gap = self.source.get(end..start).unwrap_or("");
                let own = usize::from(self.source[..end].ends_with('\n'));
                if gap.matches('\n').count() + own >= 2 {
                    self.end_line();
                    self.out
                        .push(Line::from(Span::styled(String::new(), self.base)));
                }
            }
        }
        self.depth += 1;
    }

    fn block_end(&mut self, end: usize) {
        self.depth = self.depth.saturating_sub(1);
        if self.depth == 0 {
            self.last_block_end = Some(end);
        }
    }

    fn event(&mut self, event: Event<'a>, range: std::ops::Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag, range.start),
            Event::End(tag) => self.end(tag, range.end),
            Event::Text(t) if self.code_block => {
                let code = self.code;
                let mut parts = t.split('\n').peekable();
                while let Some(part) = parts.next() {
                    // The final piece after a trailing newline is empty.
                    if part.is_empty() && parts.peek().is_none() {
                        break;
                    }
                    self.start_line();
                    self.line.push(Span::styled(format!("  {part}"), code));
                    self.end_line();
                }
            }
            Event::Text(t) => self.text(&t, self.style()),
            Event::Code(t) => self.text(&t, self.code),
            Event::Html(t) | Event::InlineHtml(t) => {
                for (i, part) in t.trim_end_matches('\n').split('\n').enumerate() {
                    if i > 0 {
                        self.end_line();
                    }
                    self.text(part, self.style());
                }
                if matches!(event_kind_is_block(&t), true) {
                    self.end_line();
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if self.table.is_some() {
                    self.cell.push(Span::styled(" ", self.style()));
                } else {
                    self.start_line();
                    self.end_line();
                }
            }
            Event::Rule => {
                self.block_start(range.start);
                self.end_line();
                self.start_line();
                self.line.push(Span::styled(
                    "─".repeat(40),
                    self.base.add_modifier(Modifier::DIM),
                ));
                self.end_line();
                self.block_end(range.end);
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "☑" } else { "☐" };
                if self.marker.is_some() {
                    self.marker = Some(mark.to_string());
                } else {
                    self.text(&format!("{mark} "), self.style());
                }
            }
            Event::FootnoteReference(t) => self.text(&format!("[^{t}]"), self.style()),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'a>, start: usize) {
        match tag {
            Tag::Paragraph => {
                self.block_start(start);
            }
            Tag::Heading { .. } => {
                self.block_start(start);
                self.end_line();
                self.push_style(Modifier::BOLD);
            }
            Tag::BlockQuote(_) => {
                self.block_start(start);
                self.end_line();
                self.quote_depth += 1;
                self.push_style(Modifier::ITALIC);
            }
            Tag::CodeBlock(kind) => {
                self.block_start(start);
                self.end_line();
                self.code_block = true;
                if let CodeBlockKind::Fenced(lang) = kind {
                    let lang = lang.split_whitespace().next().unwrap_or("");
                    if !lang.is_empty() {
                        self.start_line();
                        self.line.push(Span::styled(
                            format!("  {lang}"),
                            self.base.add_modifier(Modifier::DIM),
                        ));
                        self.end_line();
                    }
                }
            }
            Tag::List(first) => {
                self.block_start(start);
                self.end_line();
                self.lists.push(List { next: first });
            }
            Tag::Item => {
                self.end_line();
                let marker = match self.lists.last_mut() {
                    Some(List { next: Some(n) }) => {
                        let m = format!("{n}.");
                        *n += 1;
                        m
                    }
                    _ => "•".to_string(),
                };
                self.indents.push(marker.width() + 1);
                self.marker = Some(marker);
            }
            Tag::Emphasis => self.push_style(Modifier::ITALIC),
            Tag::Strong => self.push_style(Modifier::BOLD),
            Tag::Strikethrough => self.push_style(Modifier::CROSSED_OUT),
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.push_style(Modifier::UNDERLINED);
            }
            Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.push_style(Modifier::UNDERLINED);
            }
            Tag::Table(_) => {
                self.block_start(start);
                self.end_line();
                self.table = Some(Table::default());
            }
            Tag::TableHead => {
                self.push_style(Modifier::BOLD);
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.cell.clear(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd, end: usize) {
        match tag {
            TagEnd::Paragraph => {
                self.end_line();
                self.block_end(end);
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.end_line();
                self.block_end(end);
            }
            TagEnd::BlockQuote(_) => {
                self.end_line();
                self.pop_style();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.block_end(end);
            }
            TagEnd::CodeBlock => {
                self.end_line();
                self.code_block = false;
                self.block_end(end);
            }
            TagEnd::List(_) => {
                self.end_line();
                self.lists.pop();
                self.block_end(end);
            }
            TagEnd::Item => {
                // An empty item still shows its marker.
                if self.marker.is_some() {
                    self.start_line();
                }
                self.end_line();
                self.indents.pop();
                self.marker = None;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link | TagEnd::Image => {
                self.pop_style();
                let url = self.links.pop().unwrap_or_default();
                let shown: String = if self.table.is_some() {
                    self.cell.iter().map(|s| s.content.as_ref()).collect()
                } else {
                    self.line.iter().map(|s| s.content.as_ref()).collect()
                };
                // A bare autolink already shows its URL.
                if !url.is_empty() && !shown.ends_with(url.as_str()) {
                    self.text(&format!(" ({url})"), self.base.add_modifier(Modifier::DIM));
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.cell);
                if let Some(t) = &mut self.table
                    && let Some(row) = t.rows.last_mut()
                {
                    row.push(cell);
                }
            }
            TagEnd::TableHead => {
                self.pop_style();
                if let Some(t) = &mut self.table {
                    t.header_rows = t.rows.len();
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table_lines(t);
                }
                self.block_end(end);
            }
            _ => {}
        }
    }

    /// Lay a collected table out as aligned columns: header, a rule, the body.
    fn table_lines(&mut self, t: Table) {
        let width = |cell: &Vec<Span<'static>>| -> usize {
            cell.iter().map(|s| s.content.as_ref().width()).sum()
        };
        let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0usize; cols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(width(cell));
            }
        }
        let dim = self.base.add_modifier(Modifier::DIM);
        for (r, row) in t.rows.into_iter().enumerate() {
            self.start_line();
            for (i, cell) in row.into_iter().enumerate() {
                if i > 0 {
                    self.line.push(Span::styled(" │ ", dim));
                }
                let pad = widths[i].saturating_sub(width(&cell));
                self.line.extend(cell);
                if pad > 0 {
                    self.line.push(Span::styled(" ".repeat(pad), self.base));
                }
            }
            self.end_line();
            if r + 1 == t.header_rows {
                self.start_line();
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.line.push(Span::styled(rule.join("─┼─"), dim));
                self.end_line();
            }
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.end_line();
        if self.out.is_empty() {
            self.out
                .push(Line::from(Span::styled(String::new(), self.base)));
        }
        self.out
    }
}

/// Block-level HTML arrives with its trailing newline; inline HTML does not.
fn event_kind_is_block(html: &str) -> bool {
    html.ends_with('\n')
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

    fn all(text: &str) -> Vec<String> {
        render(text, base(), code()).iter().map(flat).collect()
    }

    fn one(text: &str) -> Line<'static> {
        render(text, base(), code()).remove(0)
    }

    fn has(line: &Line, text: &str, m: Modifier) -> bool {
        line.spans
            .iter()
            .any(|s| s.content.contains(text) && s.style.add_modifier.contains(m))
    }

    #[test]
    fn a_heading_is_bold_without_its_hashes() {
        let line = one("## Title here");
        assert_eq!(flat(&line), "Title here");
        assert!(has(&line, "Title", Modifier::BOLD));
    }

    #[test]
    fn hashes_without_a_space_are_not_a_heading() {
        assert_eq!(flat(&one("#nothashtag")), "#nothashtag");
        assert_eq!(flat(&one("####### seven")), "####### seven");
    }

    #[test]
    fn bullets_and_numbers_get_a_marker() {
        assert_eq!(all("- item"), ["• item"]);
        assert_eq!(all("3. third"), ["3. third"]);
        assert_eq!(all("1) one\n2) two"), ["1. one", "2. two"]);
        // A bare dash is text, not a bullet.
        assert_eq!(all("-nope"), ["-nope"]);
    }

    #[test]
    fn nested_items_indent_under_their_parent() {
        assert_eq!(
            all("- top\n  - nested\n- next"),
            ["• top", "  • nested", "• next"]
        );
        assert_eq!(all("1. a\n   - b"), ["1. a", "   • b"]);
    }

    #[test]
    fn task_items_show_a_box() {
        assert_eq!(all("- [ ] todo\n- [x] done"), ["☐ todo", "☑ done"]);
    }

    #[test]
    fn inline_code_and_emphasis_lose_their_markers() {
        let line = one("run `bwoc list` when **ready** or _later_");
        assert_eq!(flat(&line), "run bwoc list when ready or later");
        assert!(has(&line, "ready", Modifier::BOLD));
        assert!(has(&line, "later", Modifier::ITALIC));
        assert!(has(&line, "bwoc list", Modifier::REVERSED));
    }

    #[test]
    fn underscores_and_stars_inside_words_stay() {
        // The line scanner ate these: `snake_case` → "snakecase".
        assert_eq!(
            all("edit my_var_name and file_path"),
            ["edit my_var_name and file_path"]
        );
        assert_eq!(all("2 * 3 * 4 = 24"), ["2 * 3 * 4 = 24"]);
        assert_eq!(all("a `backtick"), ["a `backtick"]);
        assert_eq!(all("**bold"), ["**bold"]);
        assert_eq!(all(r"\*not italic\*"), ["*not italic*"]);
    }

    #[test]
    fn emphasis_nests() {
        let line = one("**bold with `code` inside** and *it **both***");
        assert_eq!(flat(&line), "bold with code inside and it both");
        assert!(has(&line, "code", Modifier::REVERSED));
        let both = line.spans.iter().find(|s| s.content == "both").unwrap();
        assert!(
            both.style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::ITALIC)
        );
    }

    #[test]
    fn a_link_shows_its_text_then_its_url() {
        assert_eq!(
            all("see [the docs](https://x.dev/d) now"),
            ["see the docs (https://x.dev/d) now"]
        );
        assert_eq!(all("<https://x.dev>"), ["https://x.dev"]);
    }

    #[test]
    fn a_table_lines_up_in_columns() {
        let rows = all("| File | Change |\n|---|---|\n| a.rs | **fix** |\n| long.rs | x |");
        assert_eq!(
            rows,
            [
                "File    │ Change",
                "────────┼───────",
                "a.rs    │ fix   ",
                "long.rs │ x     ",
            ]
        );
    }

    #[test]
    fn a_fenced_block_keeps_its_content_verbatim() {
        let lines = render("```rust\nlet x = *p; // **not bold**\n```", base(), code());
        assert_eq!(flat(&lines[0]), "  rust");
        assert_eq!(flat(&lines[1]), "  let x = *p; // **not bold**");
        assert_eq!(lines[1].spans[0].style, code());
        assert_eq!(lines.len(), 2);
        assert_eq!(all("~~~\nplain\n~~~"), ["  plain"]);
    }

    #[test]
    fn a_quote_is_marked_and_italic() {
        let line = one("> quoted");
        assert_eq!(flat(&line), "▏ quoted");
        assert!(has(&line, "quoted", Modifier::ITALIC));
    }

    #[test]
    fn line_breaks_and_blank_lines_are_kept() {
        assert_eq!(all("one\ntwo\n\nthree"), ["one", "two", "", "three"]);
        assert_eq!(all("# H\ntext"), ["H", "text"]);
        assert_eq!(
            all("a\n\n---\n\nb"),
            ["a", "", "─".repeat(40).as_str(), "", "b"]
        );
    }

    #[test]
    fn plain_text_survives_unchanged() {
        let text = "ปกติ — plain text with no markup";
        assert_eq!(all(text), [text]);
    }

    #[test]
    fn half_streamed_markdown_still_renders() {
        // A turn renders while it streams: an unclosed fence or table is fine.
        assert_eq!(all("```py\nprint(1)"), ["  py", "  print(1)"]);
        assert!(!all("| a | b |\n|---").is_empty());
    }
}
