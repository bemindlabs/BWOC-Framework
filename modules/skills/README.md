# `modules/skills/` — Framework Skills

**Status:** shipped. 23 framework skills.

A **framework skill** is a capability the framework recommends as a baseline for any agent: a consistent interface with verification gates that any agent can opt into. It is different from an **agent skill** (`modules/agent-template/skills/`), which an individual agent declares for itself, and from a **plugin** ([`modules/plugins/`](../plugins/)), which the workspace turns on for everyone.

A skill may depend on a plugin **kind**, never on a plugin name (for example, `scrum-via-jira` requires a `jira`-kind plugin). The dependency only runs one way: skill → plugin.

The full contract (manifest, invocation lifecycle, discovery) is in [`docs/en/SKILLS.en.md`](../../docs/en/SKILLS.en.md).

## Installed skills

| Skill | What it does |
|---|---|
| [`ai-dlc`](ai-dlc/) | Run the AI-Driven Development Life Cycle |
| [`ai-loop-engineer`](ai-loop-engineer/) | Engineer autonomous agent loops |
| [`auditor`](auditor/) | Check work or claims against a standard, surface issues, and adversarially verify each finding before reporting |
| [`counselor`](counselor/) | Meet a person with compassion |
| [`data-engineer`](data-engineer/) | Move data reliably |
| [`data-scientist`](data-scientist/) | Learn from data honestly |
| [`documenter`](documenter/) | Capture how a system actually works so the next reader/agent doesn't re-derive it |
| [`engineering`](engineering/) | Build software the disciplined way |
| [`gcloud-ops`](gcloud-ops/) | Agent-facing read-mostly GCP operations driven through the gcloud-auth + gcloud-project workflow plugins via the bwoc gcloud CLI |
| [`illustrator`](illustrator/) | Turn an intent into a visual |
| [`lawyer`](lawyer/) | Reason about rules |
| [`manager`](manager/) | Decompose work into right-sized pieces, form/assign them to Saṅgha teams + shared task lists, and track to done |
| [`mathematics`](mathematics/) | Reason rigorously with symbols and quantities |
| [`physics`](physics/) | Model a physical system |
| [`product-manager`](product-manager/) | Decide what to build and why |
| [`scrum-via-jira`](scrum-via-jira/) | Agent-facing scrum operations driven through a jira-kind plugin via the bwoc jira CLI |
| [`second-brain`](second-brain/) | Consult the fleet's harvested knowledge graph (the Second Brain) before/while working |
| [`server-rag`](server-rag/) | Ask the fleet's self-hosted RAG (semantic Q&A over server/ops docs) before re-deriving operational facts |
| [`software-engineer`](software-engineer/) | The professional software role |
| [`soul`](soul/) | Hold an agent's enduring core |
| [`systems-engineer`](systems-engineer/) | Design across components |
| [`worktree-discipline`](worktree-discipline/) | Create, isolate, and cleanup task worktrees per Anattā |
| [`writer`](writer/) | Craft written content |

## CLI

```bash
bwoc skill list | show <name>
bwoc skill verify [--run-gate]          # static check; prints each [gates].verify command
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
