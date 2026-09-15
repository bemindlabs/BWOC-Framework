---
name: incarnate
description: Scaffold a new BWOC agent with `bwoc new`, then guide the user through reviewing config.manifest.json and the remaining AGENTS.md Section 1 / persona fields. Use when the user says "incarnate", "new agent", "clone the template", or names a new agent to create.
disable-model-invocation: true
---

# /incarnate — create a new agent with `bwoc new`

This skill wraps `bwoc new` (`crates/bwoc-cli/src/new.rs`). It is user-triggered (side effects: writes the agent directory and registers it in `.bwoc/agents.toml` when a workspace resolves).

## Arguments

`$ARGUMENTS` — the agent name (required), optionally followed by `bwoc new` flags (`--target <path>`, `--role`, `--primary-model`, …).

## Steps

1. **Validate the name.** Lowercase, hyphen-separated, no spaces. Confirm with the user if it's the first time seeing this name.
2. **Run the CLI**:
   ```bash
   bwoc new <name> [flags]
   ```
   Missing required fields are prompted on a TTY; a non-TTY run fails fast listing them. Default target is `<workspace>/agents/agent-<name>` when a workspace resolves.
3. **Report what it created**: target path, symlinks, registry entry. Quote its next-steps block verbatim — do not paraphrase.
4. **Offer to fill the remaining fields** (`AGENTS.md` Section 1 scope text, `persona/README.md`). If accepted, read `modules/agent-template/config.manifest.json` and `modules/agent-template/conventions.md`, then propose values before editing.

## What this skill does NOT do

- Does not create commits or push to any remote.
- Does not overwrite an existing target directory — `bwoc new` refuses when the target exists; respect that.
- Does not modify the template itself.

## Apply the principle

Name **Yoniso manasikāra** in the report: verify the resulting agent directory by listing its contents and confirming `bwoc check <agent-path>` exits 0.
