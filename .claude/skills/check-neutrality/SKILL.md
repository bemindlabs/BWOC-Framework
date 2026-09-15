---
name: check-neutrality
description: Run the backend-neutrality audit on the agent template or a specific incarnated agent. Reports hardcoded model IDs, vendor-specific phrasing, missing symlinks, and YAML/wikilink violations in AGENTS.md. Use when the user says "check neutrality", "neutrality audit", "verify backends", or before a PR touching AGENTS.md.
---

# /check-neutrality — backend-neutrality audit

Wraps `bwoc check` (`crates/bwoc-cli/src/check.rs`). Read-only — no side effects.

## Arguments

`$ARGUMENTS` — optional path to audit (or `--all` for every agent in the workspace). Defaults to the template root.

## Steps

1. **Run the audit**:
   ```bash
   bwoc check [path]          # default here: modules/agent-template
   ```
2. **Surface violations first**, then warnings. Quote each verbatim with the file and line if the audit reports one.
3. **For each FAIL**, propose a concrete fix that preserves the two-tier format:
   - Hardcoded model ID → replace with `{{primaryModel}}` or another placeholder; update `config.manifest.json` if a new placeholder is added.
   - Backend-specific phrasing → move to Section 0 (Backend Registration) or to the per-backend symlink file.
   - YAML frontmatter or wikilinks in `AGENTS.md` → strip them; instruction files are plain Markdown only.
4. **Do not auto-apply fixes.** Wait for the user to confirm each. The `bwoc check` exit code is the source of truth; do not declare success unless it returns 0.

## When to skip running the audit

If the working set has no changes under `modules/agent-template/AGENTS.md`, `config.manifest.json`, or backend symlinks, say so and ask whether to run anyway.

## Apply the principle

Name **Samānattatā** when explaining a fix: the audit exists because all backends must be treated equally. Any per-vendor exception in `AGENTS.md` violates this.
