# 2026-09-05 — Byte budget on tool output

Closes #478. There was **no byte budget anywhere in the tool path**: `read_file`
did a whole-file `read_to_string`, `run_command` returned full stdout+stderr, and
`dispatch_rich` passed results through untouched. The only cap was `grep`'s match
count. Unbounded output is a context/cost DoS that Layer 0 *permits on an
Untrusted turn* — `read_file`/`grep` are `PURE_READ_TOOLS` precisely because
they're cheap and side-effect-free.

## What changed

- `tools/mod.rs`: `const MAX_TOOL_OUTPUT_BYTES = 64 * 1024` + `clamp_tool_output`
  — truncates on a UTF-8 **char boundary** (a naive byte slice panics once
  `sandbox`/providers run the text through `from_utf8_lossy`) and appends an
  **actionable** notice naming the next move (`read_file` with `offset`/`limit`, a
  tighter `grep`) rather than a silent cut.
- `tools/registry.rs`: clamp **once** in `dispatch_rich` right after
  `execute_rich`. That's the single seam every tool — and every MCP tool —
  passes through; on unix it runs inside the isolated child, so it also bounds
  the IPC frame. Images are untouched.
- `read_file` gains `offset` (1-based start line) + `limit` (max lines): a
  targeted slice of a large file, with a `[bwoc: lines a-b of N]` header and a
  clear past-EOF notice. Absent → whole file, unchanged.

## Decisions

- **Clamp at the seam, not per-tool.** One choke point can't be forgotten by a
  future tool and covers MCP for free — the issue's load-bearing point.
- **Never a silent cut.** A truncated read that looks complete is how an agent
  edits against content it never saw; the notice is mandatory.
- `read_file` window is **line-based** (matches how an agent pages a file);
  the byte cap still backstops a file of monstrously long lines.

## Tests

- `clamp_tool_output`: small unchanged; over-limit shrinks + announces + names
  `offset`; a wall of 3-byte chars stays valid UTF-8 (no mid-char panic).
- `read_file`: middle window header+body, whole-file unchanged, offset-past-EOF
  notice.

## Related

- Issue #478; source `research/2026-08-23_grok-build-comparison.md`.
