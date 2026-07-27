# 2026-07-27 — Release workflow: idempotent `create-release`

Fixes issue #371. The `create-release` job called `gh release create "$tag"` unconditionally; whenever a Release object for the tag already existed (tag re-push, workflow re-run, or a race), the API returned HTTP 422 `already_exists` and failed the job. Because `build` and `bump-formula` chain off it via `needs:`, they were **skipped** — so the release shipped with **zero binaries** and the Homebrew formula never bumped. Five consecutive releases were affected before the latest happened to hit the non-preexisting path.

## What changed
`.github/workflows/release.yml` — guard the create with an existence check: `gh release view "$tag" || gh release create …`. If the shell already exists, reuse it (the matrix build jobs append their assets); otherwise create it. This matches the job's own stated purpose ("make release creation happen exactly once and let matrix jobs find it pre-existing") — the code just never enforced it.

## Decisions
- **Existence-check, not `|| true`.** `|| true` would also swallow genuine failures (auth, network); the `gh release view` guard only skips the create when the object truly exists.
- **No TH pair / no version bump.** `.github/` is operator-internal (EN-only per CONTRIBUTING); a `.yml` edit doesn't trip the auto-version hook.

## Verification
`python -c 'yaml.safe_load(...)'` parses clean. Full behaviour is only observable on the next tag push (a real release run); the logic is a standard idempotent-create guard.

## Related
- Closes #371. Filed alongside the dep-vuln sweep (PR #388 quinn-proto, issue #389 rustls-webpki/rumqttc).
