# 2026-09-08 — The compatibility contract (docs for 3.0)

The code half of 3.0 landed first
(`notes/2026-09-08_schema-version-seam.md`, `notes/2026-09-08_bwoc-migrate.md`).
This is the half that says what it means. 2.x had no compatibility contract at
all: a breaking change was documented in `CHANGELOG.md` and otherwise left to be
discovered, there was no deprecation convention, no statement of which versions
are supported, and no definition of what counts as a public surface.

## What changed

- **New `docs/{en,th}/COMPATIBILITY.*`** — the three public surfaces
  (specification, on-disk schema, CLI) and, explicitly, what is *not* one; the
  table of versioned artifacts and the ones deliberately left unversioned; the
  one-major support window; the read-forward / downgrade rules; the deprecation
  sequence; supported versions.
- **New `docs/{en,th}/MIGRATION.*`** — the operator's path, with a `2.x → 3.0`
  section. Named `MIGRATION`, not `MIGRATION-3.0`: the docs naming gate in
  `.github/workflows/docs.yml` accepts only `UPPERCASE(-UPPERCASE)*.<lang>.md`,
  and one document gaining a section per major beats one file per major anyway.
- **`SECURITY.md`** gains a Supported Versions section — the latest release only,
  which is what the release model already implied and nobody had written down.
- **`docs/{en,th}/RELEASING.*`** — a "Cutting a major" checklist (the promises
  that come due, and the one-commit rule the release-pointer gates enforce), and
  the stale claim that Homebrew is not in the pipeline is removed: `release.yml`
  has had a `bump-formula` job since #460.
- **`docs/{en,th}/ROADMAP.*`** — **Phase 6 had no section in either language**,
  existing only as a sentence in the status paragraph. It has one now, and is
  declared DoD-met. **Phase 7 — *anicca*** is opened as the phase that produces
  3.0, with its deferrals (ACP #485, Dispatch #452, HV3-4/5/6, CLI surface
  reduction, code signing, crates.io) each carrying the reason they are out.
- **`VERSION.md`** — Phase 7 row and header; `docs/index.md` links both new docs.

## Decisions

- **The support window is measured in majors, not months.** "3.x reads schema 2;
  4.0 removes it." BWOC does not promise a release cadence, so a window stated
  in time would be a promise the project cannot keep.
- **The Rust API is explicitly *not* a public surface**, stated in the same table
  as the three that are. The crates are unpublished and `bwoc-core`'s modules are
  `pub` for the binaries in this workspace; saying so removes the ambiguity that
  the stale `VERSION.md` crates.io line had left sitting there.
- **Deprecation warns, it does not fail.** A deprecation that turns an existing
  installation red on upgrade day is a removal with extra steps. `bwoc check`
  reports it as a warning; the *next* major promotes it.
- **Supported versions = latest only, and the reason is structural**, not a
  policy preference: tags are cut on `main` and `CONTRIBUTING.md` forbids
  `release/*`, so supporting an older line would need branches the project chose
  not to have.
- **Phase 7 is named *anicca*.** The mechanism it builds is literally the "Anicca
  seam" the manifest code already named — formats change, and the reading of a
  format must survive the change.

## Bugs surfaced

`docs/en/MIGRATION-3.0.en.md` would have failed the `docs/<lang>/ naming` CI gate
(digits and a dot are not in `[A-Z]+(-[A-Z]+)*`). Caught by running the gate's
own `find | grep -vE` locally before committing rather than by CI.

## Related

- `docs/en/COMPATIBILITY.en.md` — the contract itself.
- `VERSION.md` §Cargo SemVer bump rules — the MAJOR trigger it formalizes.
