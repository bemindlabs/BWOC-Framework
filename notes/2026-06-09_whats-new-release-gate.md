# 2026-06-09 — What's New: refresh + release gate

The CLI "What's New" surface (`crates/bwoc-cli/src/whats_new.rs`) had silently
gone stale: its `HEADLINE` *number* is compile-derived from Cargo so it tracked
the build, but the hand-written tagline and `HIGHLIGHTS` bullets were stuck on a
months-old `gcloud IAM` release while the binary had moved several minors ahead
(to 2.29). Nothing forced the prose to follow the version.

## What changed

- **Refreshed the content to the current arc** — tagline → "OpenRouter provider
  backend" (2.29 headline), bullets now cover 2.27–2.29 (Phase 5 saṃvara,
  `RouteTarget::Gateway`, the standalone agent, OpenRouter, authenticated MQTT).
- **Added a release gate** — `highlights_cite_current_version` asserts at least
  one `HIGHLIGHTS` bullet contains the build's `MAJOR.MINOR`. The auto-version
  hook bumps the minor on release; this test then fails until someone refreshes
  the prose, so "update What's New every release" is enforced by CI rather than
  remembered.

## Decisions

- **Enforce via test, not process doc.** A failing unit test blocks the release
  build deterministically; a checklist line in CONTRIBUTING would rely on memory
  — the exact failure mode that let this rot. Mirrors the existing
  `headline_version_matches_build` guard that already ties the *number* to the
  build; this extends the same discipline to the *prose*.
- **Cite-the-version convention.** Requiring a bullet to name `MAJOR.MINOR` is a
  low-cost, deterministic signal (no fuzzy "is this prose fresh?" heuristic) and
  doubles as useful context for readers.

## Related (links)

- `crates/bwoc-cli/src/whats_new.rs`
- `crates/bwoc-cli/src/banner.rs` (imports `HEADLINE` / `HIGHLIGHTS`)
