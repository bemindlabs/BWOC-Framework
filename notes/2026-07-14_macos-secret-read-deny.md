# 2026-07-14 — macOS SBPL secret read-deny (#329, Option D)

Closed the macOS read-confinement residual by adding a selective `(deny file-read* …)` SBPL arm over a curated denylist of high-value secret paths, applied to **both** macOS read surfaces (the turn-executor jail and the tool sandbox) via one shared renderer. This narrows — does not eliminate — the residual: an *unlisted* secret is still readable, so it is documented as a narrowed residual, not Landlock parity.

## What changed

- **`jail.rs`** — new shared, `pub(crate)`, macOS-gated helpers (the canonical home for the logic):
  - `sbpl_escape` · `BWOC_SANDBOX_ALLOW_SECRET_READ_ENV` + `sbpl_allow_secret_read` (on-by-default, fail-closed escape-hatch mirroring the net arm) · `secret_read_deny_paths` + pure `secret_read_deny_paths_from` (canonicalize-existing, skip-missing, dedupe) · `sbpl_secret_read_block` (renders the deny-read lines + a re-allow per confinement root).
  - `macos_write_confine_profile` (the **turn-executor** profile — turn_executor.rs:860) now emits the secret block above its write rules.
- **`sandbox.rs`** — `build_sbpl_profile` (the tool/`run_command` sandbox, defence-in-depth on the non-jailed fallback path) delegates to the same `jail::sbpl_secret_read_block`; local duplicates removed. Refactored into a pure `build_sbpl_profile_with(worktree, secrets, allow_net, allow_secret_read)` so rendering is unit-testable without mutating process-global env.
- **Curated set:** `~/.ssh`, `~/.aws`, `~/.config/gcloud`, `~/.config/gh`, and the BWOC home (`$BWOC_HOME` else `$HOME/.bwoc` — agent keys + SessionTrust checkpoints).
- **Docs (EN + TH parity):** `docs/{en,th}/THREAT-MODEL.md` — narrowed the "macOS read / egress confinement" residual bullet + the C1 row; `jail.rs` module docs; live comments in `turn_executor.rs`. Phase 5 charter left as a historical snapshot (Anattā).
- **Tests:** pure renderer/resolver tests in both modules + a `#[ignore]` macOS `sandbox-exec` e2e proving a listed secret read is blocked while a normal worktree read and a dynamically-linked exec both succeed.

## Decisions

- **Wired into `jail.rs`, not only `sandbox.rs`.** The issue said "wire into `sandbox.rs::build_sbpl_profile`", but that builder governs only the `run_command` tool sandbox on the *non-jailed fallback* path. The turn-executor (the issue's named threat — "can read `~/.ssh`") re-execs under `jail.rs::macos_write_confine_profile`, with `run_command` children as `NoopOsSandbox` (inheriting the jail). Covering `sandbox.rs` alone would have left the primary threat untouched. Confirmed against code (turn_executor.rs:860, :1222) — the issue's "where to wire" was stale. User steered to extend to both with a shared renderer.
- **Denylist, not deny-default.** A full `(deny file-read*)` breaks the dyld shared-cache reads `sandbox-exec` needs to launch a dynamically-linked binary (verified in the issue prototype). Denylist keeps exec intact at the cost of coverage — acceptable because macOS is dev-only and egress-deny (t29) already compensates the direct-exfil path.
- **Escape-hatch `BWOC_SANDBOX_ALLOW_SECRET_READ=1`** — parity with the net arm's operator seam; on-by-default, fail-closed.
- **Trailing worktree/rw re-allow** below the denies (SBPL last-match-wins) so an overlapping secret dir can never block a confinement root the child must read.
- **Pure-core refactor** to avoid env-mutation flake: tests drive `build_sbpl_profile_with` / `secret_read_deny_paths_from` / `sbpl_secret_read_block` with explicit inputs; only the `#[ignore]` e2e touches real files.

## Status / deferred

- Verified on macOS 26.5.1 (this dev box): `cargo fmt`/`clippy` clean, 428 lib tests + 2 ignored pass, `check-deferred-fence.sh` green, bilingual parity 0 FAIL.
- No Linux verification needed — SBPL is macOS-only; the Linux read jail (Landlock) is unchanged.
- Branch `feat/harness-329-macos-secret-read-deny` → **PR #331** (squash auto-merge armed).

## Related

- Issue #329 · t29 (macOS egress parity) · jail.rs C1 (THREAT-MODEL) · sandbox.rs `SandboxExecSandbox`.
