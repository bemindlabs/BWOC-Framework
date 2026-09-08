# Version

> **Auto-maintained header.** The hook `.claude/hooks/auto-version.sh` bumps the patch number and stamps `Last-Updated` on Claude Code edits **made on the `main` branch only** — feature branches do not touch these shared lines (so concurrent PRs never collide on the version). The dev-checkpoint version advances on integration to `main` or via `scripts/bump-version.sh`. Software-Version is canonical in `Cargo.toml`; Document-Version is canonical here.

**Software-Version:** `3.0.0`   *(canonical in `Cargo.toml` — bumped on `.rs` / `.toml` edits)*
**Document-Version:** `1.14.0`   *(canonical here — bumped on `.md` edits)*
**Phase:** Phase 7 — *anicca* (versioned change & the compatibility contract), **in progress** — producing 3.0. Prior **Phase 6 — *paññā*** (harness eval & cross-platform hardening) **DoD met** *(t29–t31 shipped; t32 deep-memory sqlite-vec parked as premature — see `reports/retro/t32-deep-memory-design.md`)*. **Phase 5 — *saṃvara*** (trust-boundary & sandbox hardening) fully signed off *(t1–t9 + t11: re-exec turn-executor isolation, setrlimit, Landlock FS jail + anti-ptrace, capability gate + taint propagation, deferred-control fence, seccomp network-egress containment + the no-fd invariant)*; Phase 3 vaya + Phase 4 fleet-governance also met
**Latest release:** [`v2026.9.8-0`](https://github.com/bemindlabs/BWOC-Framework/releases/tag/v2026.9.8-0) *(2026-09-08 — **3.0.0** — **BWOC 3.0, the compatibility contract**: every artifact BWOC owns declares its schema, `bwoc migrate` moves an installation forward without losing comments or unmodeled keys, specification 3.0 is validated rather than merely written, and `[plugin].compat` is enforced with bounded ranges. 3.x reads everything 2.x wrote; schema 2 goes away in 4.0. See `docs/en/COMPATIBILITY.en.md` and `docs/en/MIGRATION.en.md`. Prior: `v2026.8.20-2` 2.44.2 — security release, a provider response can no longer choose its own provenance and an untrusted turn can no longer write its own control plane; `v2026.8.20-1` 2.44.1 — `bwoc-harness` ships in every release archive (#460))*
**Specification:** [`AGENTS.md`](modules/agent-template/AGENTS.md) v3.0
**Last-Updated:** `2026-09-08T09:32:34Z`   *(UTC, ISO 8601 — stamped on every edit)*

---

## Source of Truth

### Software-Version

Set in `Cargo.toml` at the workspace level:

```toml
[workspace.package]
version = "0.1.0"
```

All three crates inherit it via `version.workspace = true`:

- [`crates/bwoc-core`](crates/bwoc-core/Cargo.toml)
- [`crates/bwoc-cli`](crates/bwoc-cli/Cargo.toml)
- [`crates/bwoc-agent`](crates/bwoc-agent/Cargo.toml)

The hook reads from `Cargo.toml` `[workspace.package].version`, bumps the patch component on any `.rs` / `.toml` write, then mirrors the new value into the `Software-Version` line above.

### Document-Version

The `Document-Version` line above is canonical for the framework documentation set. Bumped on any `.md` write. Independent of software version — they evolve at different cadences.

Per-document frontmatter `Version` fields (e.g. `PHILOSOPHY.en.md` "Version: 2.0", `AGENTS.md` "Version: 2.0") track **specification semantic version**, which is a separate concern and is **not** touched by the hook. Those reflect breaking changes to the spec; they are bumped intentionally, not on every edit.

### Last-Updated

UTC, ISO 8601. Updated on every edit regardless of file type. Tracks last activity, not last release.

---

## Versioning Policy — Dual Namespaces

BWOC uses **two version namespaces** intentionally:

| Namespace | Scheme | Where it lives | Role |
|---|---|---|---|
| **Cargo SemVer** | `MAJOR.MINOR.PATCH` (e.g. `0.1.405`) | `Cargo.toml` `[workspace.package].version`, mirrored to `Software-Version` above | Internal development checkpoint. Auto-bumped on every Claude Code `.rs` / `.toml` edit by the hook. Tracks micro-revision granularity, not release identity. |
| **Release CalVer** | `vYYYY.M.D-<patch>` (e.g. `v2026.5.22-0`) | Git tag, GitHub Release name, release asset filename | **Public release identity.** Tag triggers [`release.yml`](.github/workflows/release.yml) to build cross-platform binaries with SHA-256 checksums. |

This is **deliberate**, not a workaround:

- Cargo SemVer with auto-bump captures every edit as a checkpoint — useful for the auto-version hook and for `bwoc --version` during development.
- Release CalVer captures *when* and *which iteration* a public artifact represents — far more legible to users than `0.1.405`.

Same-day reissues bump the patch number: `v2026.5.22-0`, `v2026.5.22-1`. CalVer alone does not encode breakage — breaking changes are still documented in `CHANGELOG.md` and bumped on the Cargo SemVer side.

### Cargo SemVer bump rules

| Bump | When |
|---|---|
| **MAJOR** | Breaking change to the framework specification (`AGENTS.md`), `config.manifest.json` schema, or any documented public CLI surface. |
| **MINOR** | New capability that does not break existing agents — new CLI command, new optional manifest field, new specification section. |
| **PATCH** | Backward-compatible fix or clarification — auto-bumped by the hook on edits made on `main` (feature branches skip the bump to avoid cross-PR version conflicts). |

The public Rust API is **not** a stable surface and the crates are **not** published to crates.io — `bwoc` ships as binaries (GitHub Releases + Homebrew). The MAJOR component tracks the on-disk and CLI contracts above, not the Rust API; the CalVer release scheme on Git tags is independent of both. (The earlier note here targeted a crates.io publish at the `1.0.0` Cargo milestone — the workspace passed that point without publishing, so the plan, not the version, is what changed.)

### Cutting a release (maintainer recipe)

```bash
# 1. Decide the CalVer tag for today's release iteration
git tag v2026.5.22-0

# 2. Push the tag — release.yml takes over from here
git push origin v2026.5.22-0

# 3. release.yml builds the matrix (Linux x86_64, macOS aarch64 + x86_64,
#    Windows x86_64), packages each as <archive>.{tar.gz,zip} with
#    .sha256 sidecar, and uploads them to the auto-created GitHub Release.
```

Same-day reissue? `v2026.5.22-1`, `v2026.5.22-2`, etc.

## Phase vs Version

**Phase** describes implementation milestones; **version** describes release identity. They are independent — Phase 1 v2.0 may span several SemVer releases (e.g. `0.1.0`, `0.2.0`, `0.3.0`) before yielding to Phase 2.

| Phase | Scope |
|---|---|
| Phase 1 v2.0 | Native Rust CLI (`bwoc`) + agent runtime (`bwoc-agent`). DoD: end-to-end **uppāda** (incarnate · check · spawn). |
| Phase 2 | **ṭhiti** commands — list, status, log, send, supervision. |
| Phase 3 | **vaya** + interconnect — stop, retire, inter-agent protocol. |
| Phase 4 | Reference agents, fleet dashboard. |
| Phase 5 | **saṃvara** — trust-boundary & sandbox hardening (re-exec isolation, Landlock/seccomp FS+network jail, capability gate). |
| Phase 6 | **paññā** — harness eval framework + cross-platform sandbox hardening. |
| Phase 7 | **anicca** — versioned change & the compatibility contract. Schema markers on every framework-owned artifact, `bwoc migrate`, an enforced specification version, and a written support window. DoD: BWOC can break its own contracts without breaking an installation silently. |

See the [README Status table](README.md#status) for current phase progress.

---

## Manual Bump (when needed)

The hook handles PATCH automatically. For MINOR / MAJOR, edit `Cargo.toml` `[workspace.package].version` directly and update this file's `Software-Version` line. For document MINOR / MAJOR, edit the `Document-Version` line here directly.

The hook does not undo manual edits to higher-order components; only the patch is auto-managed.
