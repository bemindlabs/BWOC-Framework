# Version

> **Auto-maintained header.** The hook `.claude/hooks/auto-version.sh` bumps the patch number and stamps `Last-Updated` on Claude Code edits **made on the `main` branch only** — feature branches do not touch these shared lines (so concurrent PRs never collide on the version). The dev-checkpoint version advances on integration to `main` or via `scripts/bump-version.sh`. Software-Version is canonical in `Cargo.toml`; Document-Version is canonical here.

**Software-Version:** `2.36.0`   *(canonical in `Cargo.toml` — bumped on `.rs` / `.toml` edits)*
**Document-Version:** `1.11.0`   *(canonical here — bumped on `.md` edits)*
**Phase:** Phase 6 — *paññā* (harness eval & cross-platform hardening), **t29–t31 done; t32 parked** *(t29 macOS network-egress parity in sandbox SBPL, t30 `cli` ambient-backend trust tier, t31a agent_loop decomposition, t31b eval ambient-backend guard; t32 deep-memory sqlite-vec/governance deferred as premature — see `reports/retro/t32-deep-memory-design.md`)*. Prior **Phase 5 — *saṃvara*** (trust-boundary & sandbox hardening) **DoD met — fully signed off** *(t1–t9 + t11: re-exec turn-executor isolation, setrlimit, Landlock FS jail + anti-ptrace, capability gate + taint propagation, deferred-control fence, seccomp network-egress containment + the no-fd invariant; Phase 3 vaya + Phase 4 fleet-governance also met)*
**Latest release:** [`v2026.7.23-0`](https://github.com/bemindlabs/BWOC-Framework/releases/tag/v2026.7.23-0) *(2026-07-23 — **2.36.0**; minor — **`ai-loop-engineer` skill** (engineer autonomous agent loops: design_loop + tune_loop, bounded + escalating) rounding out the skill library to 19; plus a fix correcting the skill-SPEC docs wikilink depths. Prior: `v2026.7.22-1` 2.35.0 — the skill library)*
**Specification:** [`AGENTS.md`](modules/agent-template/AGENTS.md) v2.0
**Last-Updated:** `2026-07-22T17:38:56Z`   *(UTC, ISO 8601 — stamped on every edit)*

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

Pre-1.0 (`0.x.y`) on the Cargo side means the public Rust API is not yet stable. Crates.io publish is targeted for the `1.0.0` Cargo milestone; the CalVer release scheme on Git tags is independent of that.

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

See the [README Status table](README.md#status) for current phase progress.

---

## Manual Bump (when needed)

The hook handles PATCH automatically. For MINOR / MAJOR, edit `Cargo.toml` `[workspace.package].version` directly and update this file's `Software-Version` line. For document MINOR / MAJOR, edit the `Document-Version` line here directly.

The hook does not undo manual edits to higher-order components; only the patch is auto-managed.
