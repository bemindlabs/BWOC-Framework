# 2026-06-09 — Harness git excludes-file aborts under the FS jail

`DiffSummary::from_worktree` reported an all-zero diff (a real worker's changes
lost as a no-op) on any host where `~/.config/git/ignore` exists. Root-caused to
the Phase 5 (saṃvara / C7) FS jail: git resolves `core.excludesFile` — default
`$XDG_CONFIG_HOME/git/ignore` — for `diff`/`ls-files`, but that path lies outside
the jail's allow-list. When the file *exists*, the jailed git aborts
`fatal: cannot use <path> as an exclude file` (exit 128); `git_output` then
returns `None` and the summary collapses to `{0,0,0}`. CI never saw it because a
fresh runner has no such file (git silently skips a missing excludes file).

## What changed
- `GIT_HARDENING` (crates/bwoc-harness/src/result.rs) gains
  `core.excludesFile=/dev/null`. `/dev/null` is already in the jail's rw dev
  allow-list, so git reads it (empty → no user excludes) instead of reaching for
  `~/.config/git/ignore`. A `-c` flag outranks repo-local config and the XDG
  default alike.
- New regression test `diff_summary_survives_external_excludes_file`: hermetic
  (points repo-local `core.excludesFile` at a sibling temp dir — outside the
  jail — with no global-env mutation, so it reproduces the host case on any CI).
  Verified it fails without the fix (`{0,0,0}`) and passes with it.

## Decisions
- Neutralize at the git-invocation layer, not by widening the jail allow-list.
  Adding `~/.config/git` to the jail would re-open a config-driven external-file
  read — the exact surface saṃvara closes. Pinning to `/dev/null` is in-spirit
  (Mattaññutā: fix the cause, don't grow the allow-list).

## Status / deferred
- Lib suite green (377 passed). NOT in scope: `tests/process_isolation.rs` fails
  3/3 on this host (`UnexpectedEof: failed to fill whole buffer`) — reproduced on
  the clean 2.28.0 baseline, so it is pre-existing and environment-specific, not
  caused by this change. Tracked separately (one concern per PR).

## Related
- Phase 5 saṃvara jail: #238; this is a follow-up hardening-correctness fix.
