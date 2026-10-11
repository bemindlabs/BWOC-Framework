# 2026-10-11 — Eleven vendor-CLI backends (pi, omp, devin, droid, opencode, kilo, hermes, qodercli, qwen, cursor, mastracode)

Added as vendor-CLI backends, modelled on the grok commit (76ae9c4): `Backend` variants, entry-file symlinks, effort flags, model suggestions, `bwoc run` arms, docs counts (ten -> twenty-one declared). kimi already existed.

## Decisions
- Fail-safe headless posture: no bypass flag anywhere (`--auto`, `--yolo`, `--force`, `--skip-permissions-unsafe`).
- Real `bwoc run`: droid (`exec --auto low --cwd`), opencode / kilo (`run --`, no `--auto`), qodercli (`-p`, auto-deny, `--max-turns 50`), mastracode (`--permission-mode deny`).
- Deferred (`HeadlessUnsupported`; interactive spawn still works): pi, omp (no sandbox/permission layer), devin, qwen (headless approval behaviour unverified), hermes (`-z` bypasses approvals), cursor (`agent -p` only proposes unless `--force`).
- `cursor` backend id maps to the binary `agent` (generic name; documented). `qodercli` binary name unverified (install docs say `qoder`).
- Effort: pi `--thinking`, mastracode `--thinking-level` on spawn; droid `-r` and opencode `--variant` only in the headless arm.

## Unverified
Only `opencode` was installed (`run --` verified for leading-dash tasks). All other flags come from vendor docs.
