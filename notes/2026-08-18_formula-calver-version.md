# 2026-08-18 — Homebrew formula version keyed on CalVer tag

`scripts/bump-formula.sh` derived the formula's `version` from the Cargo
workspace SemVer. That SemVer only advances on `.rs`/`.toml` edits, so
consecutive CalVer releases could share one SemVer — the formula stayed at the
same `version` and `brew upgrade` never detected a new build. Copilot flagged
this on #435, which merged with the bug. This fixes it.

## What changed

- `scripts/bump-formula.sh`: `version` now comes from the release **tag**,
  normalized to dotted-numeric — `v2026.8.12-0` → `2026.8.12.0` (strip `v`,
  turn the `-<patch>` separator into `.`). Unique per release and monotonic in
  Homebrew's comparator, including same-day re-issues (`-1` → `.1`).
- Removed the now-dead `cargo_toml` variable (definition + existence check).
- `Formula/bwoc.rb`: corrected the already-shipped `version "2.42.0"` →
  `2026.8.12.0` so the current release's `brew upgrade` works (`2` < `2026`, so
  Homebrew sees an upgrade). URLs/sha256 were already correct for v2026.8.12-0.

## Decisions

- Kept the `test do` block asserting `bwoc --version` by substring, not by the
  formula `version` — the CLI reports the Cargo SemVer, a deliberately
  different scheme from the CalVer formula version. The existing comment
  already documents that split.

## Related

- Fixes the Copilot finding on #435 (formula version stuck at 2.42.0).
