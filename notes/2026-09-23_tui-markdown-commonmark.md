# 2026-09-23 — The chat TUI parses Markdown as CommonMark

The owner reported that assistant Markdown still showed raw. A real TUI session fed a sample reply confirmed it, and also showed text being lost: `markdown.rs` was a line scanner that treated any `_` or `*` as emphasis, so `snake_case` came out as `snakecase` and `2 * 3 * 4` as `2  3  4`. Links, tables, `1)` lists, task boxes, escapes, rules, `~~~` fences and nested emphasis stayed raw.

## What changed

- `crates/bwoc-tui/src/markdown.rs` is rewritten on `pulldown-cmark` 0.13 (`default-features = false`) with tables, task lists and strikethrough enabled. The public `render(text, base, code)` is unchanged, so the caller is untouched.
- Mapping: headings and strong text are bold, emphasis is italic, strikethrough is crossed out, and code uses the code style. Quotes get a `▏` bar. Lists use `•` or `N.` markers, nested items indent under their parent's text, and task items show `☐`/`☑`. Links show their text followed by `(url)` dimmed, unless the text already is the URL. Tables become aligned columns with a `─┼─` rule under the header. `---` becomes a dim rule. A code block shows its language on a dim line and then its lines, indented.
- Soft breaks stay as line breaks, and a blank line in the source between top-level blocks stays a blank row. The source's own line layout is kept rather than reflowed.

## Decisions

- **The parser dependency is taken on.** The original module avoided one for Mattaññutā. The owner approved reversing that, because CommonMark's emphasis rules (intraword `_`, flanking, nesting, escapes) are what a hand-written scanner kept getting wrong, and a list of patches would grow without end. The dependency stays in `bwoc-tui`; `bwoc-core` stays lean.
- `pulldown-cmark`'s range for a block includes its trailing newline, so the blank-row check counts that newline together with the gap between blocks.

## Verified

- 15 renderer tests, including the cases that lost text, nesting, tables, task boxes, links and half-streamed input. `bwoc-tui` passes 84/84 and clippy is clean.
- A real TUI session against a mock endpoint that streams the sample renders every case correctly.
