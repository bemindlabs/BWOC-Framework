# 2026-05-25 — `inbox --all --watch` (#46) + formula auto-bump CI (#52)

Two fleet-requested features built by agent-pi in one run, designed + reviewed by agent-oracle. Logged together as one session note (the pair shipped in the same pi pass). Both observe/automation work; no behaviour change to existing commands.

## What changed

- **#46 — `bwoc inbox --all --watch`** (`crates/bwoc-cli/src/inbox.rs`): a fleet-wide merged live message stream. Lifts the prior `--all`+`--watch` refusal (`--clear` stays refused under `--all` — mass-clear is too destructive). The single-inbox tail core was extracted into `read_complete_lines_from`, shared by the single `--watch`, its `--json` variant, and the merged tail — one watcher, not two. Per-agent `--limit` backlog, then 300 ms poll across every registry inbox; new envelopes emitted tagged with recipient in arrival order (no global sort); `--json` adds a `recipient` field; a missing inbox is skipped, not an error.
- **#52 — formula auto-bump on release-tag publish** (`.github/workflows/release.yml` + `scripts/bump-formula.sh`): a `bump-formula` job (`needs: build`) rewrites `Formula/bwoc.rb` from the just-published release and commits it to the default branch, so the Homebrew tap can never go stale again (the class of bug that left it pinned at 2.0.0 while releases reached 2.4.0 — manually fixed in #51, permanently fixed here).

## Decisions

- **#52 logic lives in `scripts/bump-formula.sh`, not inline YAML** — testable locally without pushing a tag (pi validated it with synthetic sidecars + error cases + `actionlint`/`shellcheck`).
- **Owner/repo in the formula URLs is preserved from the existing formula, not derived from `$GITHUB_REPOSITORY`** — the release host (`bemindlabs/BWOC-Framework`) differs from the CI repo / `Cargo.toml` `repository`; deriving it would silently break the download host. Sharp catch by pi.
- **`version` ← `Cargo.toml` `[workspace.package].version`** (SemVer the CLI reports); the four URL fragments ← `${GITHUB_REF_NAME}` (CalVer tag); the four `sha256` ← the `.tar.gz.sha256` sidecars (Windows `.zip` sidecar ignored).
- **Direct-commit-to-default-branch** (not PR): the tap serves `HEAD`, the release is already cut, and the bump is mechanical + in-job verified. `github-actions[bot]` identity; idempotent no-op skip when already current. **Verified `main` is not branch-protected**, so the bot push works.

## Status / deferred

- **#52 caveat:** if `main` branch-protection is added later, the auto-bump push needs the bot allowlisted or a switch to PR-mode. Documented assumption, not a current blocker.
- **Staleness guard** (the optional secondary in #52) intentionally **not** added — the auto-bump is the actual fix; a guard would add a second CI surface (Mattaññutā). Easy to add later for defense-in-depth.

## Related (links)

- GH #46, #52; #51 (manual 2.4.0 formula bump this automates), #3 (stale-install observation), #44 (runtime update-check — the sibling)
- `crates/bwoc-cli/src/inbox.rs`, `.github/workflows/release.yml`, `scripts/bump-formula.sh`, `Formula/bwoc.rb`
