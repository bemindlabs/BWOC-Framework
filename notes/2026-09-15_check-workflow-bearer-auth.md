# 2026-09-15 — `bwoc check` accepts bearer `[auth]` for workflow plugins

`bwoc check --all` flagged the shipped `workflow/accounting-api` plugin with "[sources] table missing". `audit_workflow_auth` applied gcloud's `[sources]` schema (notes/2026-05-28_gcloud-workflow-plugin-architecture.md §Decision 3) to every workflow-kind plugin. accounting-api uses a bearer `[auth]` table instead, and `accounting.sh` resolves exactly that: env var first, then the key file.

## What changed
- `audit_workflow_auth` accepts either shape. The `[sources]` validation is unchanged. A new `audit_workflow_bearer_auth` checks `scheme = "bearer"`, `env_var` against `^[A-Z][A-Z0-9_]*$`, `key_file` as a relative path under `.bwoc/secrets/` with no `..`, and optional `[auth.scopes]` as a table of strings.
- The undeclared-key secret-leak guard is now the shared `reject_undeclared_shape_keys`, used by both shapes.
- If `[auth]` is present it is always audited, even alongside `[sources]`, so a stray `[auth]` can't hide a value.
- An `auth.toml` with neither table gets a violation that names both accepted shapes.

## Decisions
- The existing guard is structural: it allows only declared keys and never echoes a value. jira and figma use empty placeholders instead, so no value-sniffing helper existed to reuse. The bearer shape has no placeholders, so it reuses the structural guard. The env-var-name pattern also rejects a pasted key value.
