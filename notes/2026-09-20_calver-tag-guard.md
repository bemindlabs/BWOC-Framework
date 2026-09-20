# 2026-09-20 — Fixing the CalVer slip, and making it unrepeatable

3.5.0 was tagged `v2026.9.21-0` on **2026-09-20**. The date was typed from memory instead of read from the clock.

## Why it could not simply be corrected
A published tag is load-bearing: its assets are downloaded under that name, and `scripts/bump-formula.sh` turns the tag into the Homebrew formula's `version`. Re-cutting 3.6.0 as `v2026.9.20-1` would have sorted *below* the 3.5.0 already shipped, so `brew upgrade` would not have offered it. 3.6.0 therefore stayed on the wrong date (`v2026.9.21-1`) and the calendar catches up at the next cut.

## What changed
- **`scripts/next-tag.sh`** derives the tag instead of a human typing it: today's date, first free patch — and, while a published tag is dated ahead of today, it stays on that date so tags keep sorting upward. `--check <tag>` validates one.
- **`release.yml` runs `--check` before anything is published**, so a tag more than a day from the cut fails the release rather than shipping under a name that can never be fixed.
- **`CHANGELOG.md`** now dates both 3.5.0 and 3.6.0 as 2026-09-20 — the truth — with a note explaining the `v2026.9.21-*` tag names.
- **`CONTRIBUTING.md`** points at the script where it describes the tag scheme.

## Decisions
- **Fix forward, never re-tag.** Deleting a published tag breaks every checksum someone already fetched.
- **±1 day of slack** in the check: a maintainer in +07 cutting at 01:00 is a day ahead of UTC legitimately; five days ahead is a typo.
- The guard lives in the release workflow, not a pre-commit hook: the tag push is the moment that matters.
