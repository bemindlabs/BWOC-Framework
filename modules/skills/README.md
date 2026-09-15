# `modules/skills/` — Framework Skills

**Status:** shipped. 5 framework skills.

A **framework skill** is prose guidance (`SPEC.md`) plus a manifest that an agent opts into. No runtime loads skills; the `bwoc skill` commands audit and list them. It is different from an **agent skill** (`modules/agent-template/skills/`), which an individual agent declares for itself, and from a **plugin** ([`modules/plugins/`](../plugins/)), which the workspace turns on for everyone.

A skill may depend on a plugin **kind**, never on a plugin name (for example, a skill that drives Jira declares `requires_plugins = ["jira"]`). The dependency only runs one way: skill → plugin. None of the shipped skills declares one today.

The full contract (manifest, invocation lifecycle, discovery) is in [`docs/en/SKILLS.en.md`](../../docs/en/SKILLS.en.md).

## Installed skills

| Skill | What it does |
|---|---|
| [`ai-dlc`](ai-dlc/) | Run the AI-Driven Development Life Cycle |
| [`auditor`](auditor/) | Check work or claims against a standard, surface issues, and adversarially verify each finding before reporting |
| [`documenter`](documenter/) | Capture how a system actually works so the next reader/agent doesn't re-derive it |
| [`engineering`](engineering/) | Build software the disciplined way |
| [`second-brain`](second-brain/) | Consult the fleet's harvested knowledge graph (the Second Brain) before/while working |
## CLI

```bash
bwoc skill list | show <name>
bwoc skill verify [--run-gates]         # static check; prints each [gates].verify command
bwoc skill init <name>                  # scaffold from modules/skill-template/
bwoc skill install <path|git|tarball>   # SHA-256 trust gate
bwoc skill enable|disable <name>        # on the current agent
bwoc skill remove <name>
```

## Distinction from `.claude/skills/`

- **`.claude/skills/<name>/SKILL.md`** are Claude Code project skills: slash commands for working on *this repo* in a Claude Code session.
- **Framework skills** are agent-runtime capabilities. An incarnated agent uses them during its own operation, whichever backend drives it.

## See also

- [`modules/README.md`](../README.md)
- [`docs/en/SKILLS.en.md`](../../docs/en/SKILLS.en.md)
- [`modules/agent-template/skills/SPEC.md`](../agent-template/skills/SPEC.md) — the per-agent skill slot (distinct)
