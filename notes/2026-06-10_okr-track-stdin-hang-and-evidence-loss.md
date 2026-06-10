# 2026-06-10 — okr track: stdin hang (#279) + silent as_of/evidence loss (#278)

Two `cmd_track` bugs in `modules/plugins/okr/workspace-okrs/okr.sh`, both reported from a
downstream workspace review, both fixed in one PR (same function, adjacent root causes).

## What changed

- **#279 — stdin gate.** `track` read stdin whenever it was non-TTY (`[[ ! -t 0 ]] → cat`),
  so any open-but-idle stdin (CI step, cron, background job, wrapping pipeline) blocked
  forever in `cat`. The read is now gated on the *dispatcher contract* (SPEC §protocol):
  stdin is consulted only when there is **no argv** AND **`BWOC_OKR_OPERATION` is set** —
  exactly the two signals `bwoc okr` (BWOC-48, `crates/bwoc-cli/src/okr.rs`) sends. Hand/argv
  invocations never touch stdin.
- **#278 — tail flush.** The rewrite `awk` dropped any existing `as_of` unconditionally but
  re-emitted it (and applied `--evidence`) only *inside the existing-`evidence`-line branch*.
  A KR block without an `evidence` line therefore lost `as_of` silently and ignored
  `--evidence`, exit 0. The awk now carries a `flush_tail()` that emits the requested
  evidence (if `setev`) + the re-stamped `as_of` at the **end of the target block** (next
  `[[key_result]]` header or `END`), with `tail_done` preventing double emission when the
  block did have an evidence line (that inline path is unchanged).

## Decisions

- Gate stdin on `BWOC_OKR_OPERATION` rather than a `read -t 0` poll: the env marker is the
  documented dispatcher signal and is deterministic; a readiness poll still races a slow
  producer.
- Flushed tail lands after any trailing blank line of the block (before the next header) —
  still the same TOML array element; placement cosmetics not worth extra awk state.

## Bugs surfaced and fixed

Verified by hand in the worktree (no existing shell-test harness for plugins):
argv+idle-stdin completes (was SIGALRM); no-evidence KR gains
`evidence = { kind, value }` + fresh `as_of`; evidence-line KR unchanged behavior;
dispatcher env+stdin-JSON path works; argv without `--evidence` stamps `as_of` only;
`report` parses the rewritten multi-block file (2 KRs).

## Related

- Fixes #278, fixes #279.
- SPEC: `modules/plugins/okr/workspace-okrs/SPEC.md` (dispatcher contract).
