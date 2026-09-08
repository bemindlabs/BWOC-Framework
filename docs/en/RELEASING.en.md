---
title: Releasing BWOC
aliases:
  - Release Process
tags:
  - group/framework
  - type/process
  - meta/operations
parent: English
nav_order: 7
---

# Releasing BWOC

> [!abstract] How to cut a release of the bwoc toolkit (`bwoc` CLI + `bwoc-agent` daemon + `bwoc-harness` agentic loop). Releases are tag-driven: pushing a CalVer tag like `v2026.5.22-0` triggers the cross-platform build + GitHub Release upload pipeline.

## Dual versioning — what to read first

BWOC uses **two version namespaces deliberately**:

| Namespace | Scheme | Where | Role |
|---|---|---|---|
| Cargo SemVer | `0.1.405` | `Cargo.toml` workspace + `Software-Version` | Internal dev checkpoint. Auto-bumped on every Claude Code `.rs` / `.toml` edit. |
| Release CalVer | `v2026.5.22-0` | Git tag, GitHub Release, asset filenames | **Public release identity.** Tag triggers `release.yml`. |

Full policy in [`VERSION.md`](../../VERSION.md) §"Versioning Policy — Dual Namespaces". The short version: Cargo SemVer is a dev checkpoint; the public release name is CalVer.

## Pre-flight

Before tagging, the maintainer should verify:

- [ ] **CI is green** on `main` for the most recent commit — see [Actions → CI](https://github.com/bemindlabs/BWOC-Framework/actions/workflows/ci.yml).
- [ ] **`CHANGELOG.md`** has a section for the CalVer tag you're about to push. Rename the existing `[Unreleased]` to `[v2026.5.22-0] — 2026-05-22` and create a new empty `[Unreleased]` above it.
- [ ] **`VERSION.md`** auto-updates on edits; nothing to manually bump for a release.
- [ ] **No uncommitted changes** — release artifacts should reflect a clean tree.

## Cut the tag

Choose today's CalVer tag — `vYYYY.M.D-<patch>`, where patch starts at 0 and increments for same-day re-issues:

```bash
git tag v2026.5.22-0
git push origin v2026.5.22-0
```

The tag matches the workflow's filter (`v[0-9][0-9][0-9][0-9].*`) and triggers [`.github/workflows/release.yml`](../../.github/workflows/release.yml):

1. **Matrix build** — 5 release-mode targets in parallel:
   - `x86_64-unknown-linux-gnu` (Linux x64)
   - `aarch64-unknown-linux-gnu` (Linux ARM64, native on `ubuntu-24.04-arm` runner)
   - `aarch64-apple-darwin` (macOS Apple Silicon)
   - `x86_64-apple-darwin` (macOS Intel)
   - `x86_64-pc-windows-msvc`
2. **Package** each as `bwoc-<tag>-<target>.{tar.gz|zip}` containing `bwoc`, `bwoc-agent`, `bwoc-harness`, `README.md`, `LICENSE`, `CHANGELOG.md`. The packaging step asserts all three binaries are present and fails the build otherwise (#460).
3. **Sidecar** a `.sha256` next to each archive.
4. **Auto-create GitHub Release** with notes generated from the commit range since the previous tag.
5. **Upload** all artifacts. `fail_on_unmatched_files: true` aborts the workflow if any archive is missing — partial releases never ship.

## Same-day re-issue

Bump the patch number, not the date:

```
v2026.5.22-0    # first release of the day
v2026.5.22-1    # re-issue (e.g. broken artifact pulled, fix forward)
v2026.5.22-2    # second re-issue
```

This keeps the date stable while making the iteration explicit.

## Prerelease vs stable

CalVer tags **always** contain `-<patch>`, so the workflow can't auto-detect prerelease from the tag shape (the way SemVer tags like `v0.1.0-rc1` do). Every CalVer release is treated as **stable** by default; flip the GitHub Release's "Set as a pre-release" toggle by hand for genuinely experimental builds.

In practice you rarely need this — same-day patch bumps cover most "release something quickly" cases without the prerelease label.

## Cutting a major

A MAJOR release is not a bigger tag — it is a set of promises coming due. Before
the version bump, everything below has to be true; `VERSION.md` §Cargo SemVer
bump rules defines what makes a release major in the first place.

1. **Every breaking change is named** in `CHANGELOG.md` under the release, with
   what replaces it.
2. **The migration path exists and is tested.** `bwoc migrate` covers every
   changed on-disk format, `crates/bwoc-cli/tests/migrate_roundtrip.rs` proves it
   round-trips without losing unmodeled keys, and `docs/{en,th}/MIGRATION-<X.0>.*`
   walks an operator through it.
3. **The previous schema still reads.** One major of overlap
   ([`COMPATIBILITY.en.md`](COMPATIBILITY.en.md#the-support-window)) — the new
   binary reads the old artifacts, warns, and names the migration command.
4. **Deprecations promised for this major are actually removed**, and warnings
   promised for the *next* one are in place.
5. **Plugin `compat` ranges are re-declared** to the new major across
   `modules/**/manifest.toml` — bounded above. Do this in the same commit as the
   version bump: a bounded range for a version that has not landed yet refuses
   every plugin on an intermediate `main`.
6. **The pointers agree.** `crates/bwoc-cli/tests/release_pointers.rs` gates
   `Cargo.toml`, `README.md`, `VERSION.md` and `Formula/bwoc.rb` against the top
   CHANGELOG entry, and `whats_new.rs` fails unless a `HIGHLIGHTS` bullet cites
   the new MAJOR.MINOR. All of it lands in one commit or CI is red in between.
7. **`VERSION.md` §Specification** matches the `| **Version** |` row in
   `modules/agent-template/AGENTS.md`.

## What's NOT in the pipeline yet

- **Code signing** — Apple notarization (macOS) and Windows Authenticode are not configured. Binaries ship unsigned with SHA-256 checksums; users see "untrusted developer" prompts on first launch. Adding signing requires the maintainer to provision certs and store keys in GitHub Actions secrets.
- **Linux musl** — `aarch64-unknown-linux-gnu` ships; `x86_64-unknown-linux-musl` (and `aarch64-unknown-linux-musl`) can be added when there's user demand for distros without glibc (Alpine, distroless containers).
- **Scoop manifest / cargo binstall metadata** — distribution-system integrations live in their own ecosystems. (Homebrew is no longer on this list: `release.yml` has a `bump-formula` job that opens a formula-bump branch after every tag, and `Formula/bwoc.rb` ships `bwoc`, `bwoc-agent` and `bwoc-harness`.)

## Rolling back

If a tagged release ships broken artifacts:

1. **Don't** delete the tag — the GitHub Release retains the broken binaries, and users may already have downloaded them. Same-day re-issues exist so the timeline is auditable.
2. Cut a new same-day patch (e.g. `v2026.5.22-1` after `v2026.5.22-0`) with the fix.
3. Edit the broken release's notes to point at the replacement.

CalVer's monotonic ordering keeps the rollback simple: the highest patch suffix on the latest date is the canonical "current" build.

## See also

- [`.github/workflows/release.yml`](../../.github/workflows/release.yml) — the workflow this doc explains.
- [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) — the per-commit gate that should be green before tagging.
- [`CHANGELOG.md`](../../CHANGELOG.md) — what to update before tagging.
- [`VERSION.md`](../../VERSION.md) — current version, dual-namespace policy, manual-bump rules.
- [`COMPATIBILITY.en.md`](COMPATIBILITY.en.md) — what a release may break, and the support window.
- [`ROADMAP.en.md`](ROADMAP.en.md) — phases (does not determine version).
