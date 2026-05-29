---
title: Remove RELEASE_PAT from release.yml — collapse to the GITHUB_TOKEN path
date: 2026-05-29
status: done
related: ["#101 (fix: don't fail the run when RELEASE_PAT is unset)", "#52 (keep the Homebrew tap honest)"]
---

# 2026-05-29 — Remove RELEASE_PAT from release.yml

The `bump-formula` job carried an opt-in zero-touch path keyed on the
`RELEASE_PAT` secret: when set, it would open + auto-merge the Homebrew
formula-bump PR under a real identity; when unset, it fell back to
`GITHUB_TOKEN`, pushed the branch, and printed a manual finish command.

Audit of the live repo showed the PAT path was **dead in practice**:

- `gh secret list` → only `CLAUDE_CODE_OAUTH_TOKEN` exists; `RELEASE_PAT` was
  never configured.
- Every `chore(formula)` PR (#51, #59, #63, #69, #112, #115) was authored **and**
  merged by `kla-bemindlabs`, not `github-actions[bot]` — i.e. the fallback
  branch+manual flow ran every release; `gh pr create`/`gh pr merge` never fired.

So the `secrets.RELEASE_PAT || secrets.GITHUB_TOKEN` expressions always resolved
to `GITHUB_TOKEN`, and the `HAS_PAT` branch always took the manual path.

## What changed

- Checkout `token:` → `${{ secrets.GITHUB_TOKEN }}` (dropped the `|| RELEASE_PAT`).
- Removed the `HAS_PAT` env and the conditional, plus the unreachable
  `gh pr create` + `gh pr merge --auto` tail.
- The step (renamed *"Push a formula-bump branch if the formula changed"*) keeps
  its early `exit 0` when `Formula/bwoc.rb` is unchanged; when the formula did
  change it now always takes the push-branch + print-finish-command path — no
  longer gated on `HAS_PAT` — then exits 0, exactly what already happened every
  release. (What became unconditional is that path, not the push itself.)
- Rewrote the three explanatory comments to stop advertising RELEASE_PAT.

Net behavior is **unchanged** — this removes a dead branch, not a working
feature. The operator still opens + auto-merges the formula PR by hand, as
before. `.yml`-only edit, so no auto-version bump.

## Decisions

- **Delete rather than keep dormant.** Mattaññutā — an opt-in hook that was
  never wired and silently shadowed by the fallback earns no line. If hands-off
  formula bumps are wanted later, re-add a PAT-or-`actions:write`-app path then.

## Alternatives considered

- *Keep RELEASE_PAT, just document it better* — rejected; the secret was never
  set in two-plus weeks of releases, so the simpler workflow wins.

## Status / deferred

- Fully hands-off release (auto-open + auto-merge the formula PR) remains a
  future option, now requiring an explicit re-add of an identity that can both
  open a PR and trigger CI under the org's Actions-PR restriction.
