# 2026-07-14 — macOS turn-executor egress-deny (t29 follow-up)

Extended `(deny network*)` to the **turn-executor jail** (`jail.rs::macos_write_confine_profile`), closing the egress gap on the primary macOS jailed path. t29 had wired egress-deny only into `sandbox.rs::build_sbpl_profile` (the tool-sandbox / fallback layer) and explicitly deferred the executor profile as "a follow-up if macOS is ever treated as more than a dev box" (see `notes/2026-06-12_t29-macos-egress-parity.md`). This is that follow-up. Same shape as the #329 read-deny extension shipped earlier today.

## Why now (not "when macOS is prod")

The #329 read residual (merged in PR #331 earlier today) documents "egress-deny (t29) already compensates the direct-exfil path." That compensation was **not actually true on the primary jailed path**: on macOS the executor re-execs under `jail.rs::macos_write_confine_profile` (no `(deny network*)`), and its `run_command` children run as `NoopOsSandbox` (nested `sandbox-exec` is forbidden), so neither the executor nor its tool subprocesses were egress-contained — only the non-jailed fallback (`sandbox.rs`) was. An executor could read an *unlisted* secret and open a socket to exfil it. Extending egress-deny to the executor makes the #329 compensation claim genuinely hold. Per t29's own ruling, `(deny network*)` is the **sole control-A** on macOS (no seccomp), so this line is load-bearing.

## What changed

- **`jail.rs`** — net helpers moved here as the canonical, shared home for macOS SBPL primitives (alongside the #329 secret helpers): `BWOC_SANDBOX_ALLOW_NET_ENV`, `sbpl_allow_net`, `sbpl_net_rule(allow_net)`. `macos_write_confine_profile` now emits the net arm above the file-write rules (order: `allow default` → net → secret read-deny → write rules).
- **`sandbox.rs`** — deleted its local net const/fn; `build_sbpl_profile` + `build_sbpl_profile_with` delegate to `jail::sbpl_net_rule` / `jail::sbpl_allow_net`. Existing net tests keep their bare `BWOC_SANDBOX_ALLOW_NET_ENV` name via a `use crate::jail::…` import. One shared renderer ⇒ the two macOS surfaces cannot drift.
- **Docs (EN + TH parity):** THREAT-MODEL residual bullet now says egress-deny is enforced on **both** surfaces (executor jail + tool sandbox), closing t29's follow-up; `jail.rs` module docs, the `WriteConfineOnly` status doc, and `turn_executor.rs` comments updated.
- **Tests:** pure `sbpl_net_rule` toggle test + a host-independent structural test that the executor profile carries exactly one net arm and, when denying, places it above the write rules. Env-parsing behaviour stays covered by the existing `sandbox.rs` net tests (now importing the const from `jail`).

## Decisions

- **Net helpers live in `jail.rs`, sandbox delegates** — symmetric with the #329 secret refactor; `jail.rs` is the lower-level canonical confinement module. Mechanical rename of test refs, no behaviour change.
- **Posture parity, not flag parity** (Samānattatā) — on-by-default, fail-closed, `BWOC_SANDBOX_ALLOW_NET=1` test/operator seam only; unchanged from t29's ruling, now applied to the executor too.

## Status / deferred

- Verified on macOS 26.5.1: fmt/clippy clean, 430 lib tests + 2 ignored pass, deferred-fence green, bilingual parity 0 FAIL.
- Linux unaffected (seccomp egress arm unchanged).
- Branch `feat/harness-macos-executor-egress-deny`; PR pending.

## Related

- `notes/2026-06-12_t29-macos-egress-parity.md` (the deferral) · `notes/2026-07-14_macos-secret-read-deny.md` (#329, the read sibling) · PR #331 · THREAT-MODEL C1 + Residuals.
