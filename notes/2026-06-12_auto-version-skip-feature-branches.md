---
title: auto-version hook — skip bump on non-main branches
date: 2026-06-12
tags:
  - type/note
  - area/tooling
---

# auto-version.sh now bumps only on `main`

## What changed

`.claude/hooks/auto-version.sh` gained a branch guard: after the self-managed-
files guard, it `exit 0`s when the current branch is not `main` (detached HEAD /
git-less checkout also counts as "not main" — fail-closed). VERSION.md prose
updated to match.

## Why

The hook bumped `Cargo.toml [workspace.package].version` + VERSION.md's
Software/Document-Version on **every** `.rs`/`.toml`/`.md` edit, on whatever
branch the edit happened. Those are a single shared mutable line, so as soon as
two PRs were open at once, the first to merge left the others **DIRTY** —
conflicting on exactly that line. Observed repeatedly across the Phase 6 PRs
(t30, t31a, t31b each needed a manual `rebase` + `--theirs VERSION.md` + resolve
Cargo.toml/Cargo.lock). The per-edit auto-bump bought micro-checkpoint
granularity that nobody consumed, at the cost of a manual rebase per follow-up
PR.

## Effect

Feature branches no longer touch the version files, so concurrent PRs never
collide there. The dev-checkpoint version advances on edits made directly on
`main`, or via `scripts/bump-version.sh` / the `.bwoc/next-bump.*` sentinels at
release/integration time. Since day-to-day work happens on branches (main is
protected), this effectively makes the version **release-managed** rather than
per-keystroke — which matches the Release-CalVer philosophy already documented
in VERSION.md.

## Alternatives considered

- **Exclude the version line from PR diffs** — no clean git mechanism; the
  working-tree change still exists and would re-conflict.
- **A CI step that bumps on merge to `main`** — heavier (needs write-back from
  CI); the branch-guard achieves the no-conflict goal with one `if`.
- **Keep manual-rebasing** — the friction was real and recurring; the owner
  chose the hook fix.

## Status

`bash -n` clean; pipe-tested: a `.rs` edit on a non-main branch produces no bump
and leaves VERSION.md/Cargo.toml untouched; the `main` gate passes through to the
unchanged bump logic.
