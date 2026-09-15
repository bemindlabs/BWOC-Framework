---
title: Agent Capabilities
aliases:
  - Capabilities
  - Skill Registry
tags:
  - group/agents
  - type/design
  - meta/template
---

# Agent Capabilities

> [!abstract] Skill and expertise declaration for multi-agent routing and discovery. Reference material: no framework code parses this file — the A2A agent card is built from `config.manifest.json` fields.

## Capability Manifest Format

```json
{
  "agentId": "agent-{{name}}",
  "role": "{{agentRole}}",
  "skills": [
    {
      "id": "{{skill-id}}",
      "name": "{{Skill Name}}",
      "proficiency": "expert | advanced | intermediate | novice",
      "domains": ["{{domain-1}}", "{{domain-2}}"]
    }
  ],
  "expertiseDomains": ["{{domain-1}}", "{{domain-2}}"],
  "vetoDomains": [],
  "votingWeight": 1,
  "canDelegate": false,
  "canReview": true
}
```

## Fields

| Field | Required | Description |
|---|---|---|
| `agentId` | yes | Agent identifier |
| `role` | yes | One-line role description |
| `skills` | yes | List of skill objects |
| `skills[].proficiency` | yes | `expert`, `advanced`, `intermediate`, `novice` |
| `expertiseDomains` | yes | Domains for routing decisions |
| `vetoDomains` | no | Domains where agent has veto authority |
| `canDelegate` | no | Whether agent can spawn sub-agents (default: false) |
| `canReview` | no | Whether agent can review other agents' work |

> [!warning] Principle of Least Privilege
> Most agents should have `canDelegate: false`. Only orchestrator-role agents explicitly designed for coordination should declare delegation authority.

## Proficiency Levels

| Level | Meaning |
|---|---|
| `expert` | Consistently high success rate, low revision cycles |
| `advanced` | Solid capability, occasional need for review |
| `intermediate` | Functional but benefits from oversight |
| `novice` | Learning — requires review on most tasks |

## Usage

Human or agent orchestrators can consult capabilities to:
- Route tasks to the best-matched agent
- Determine review authority
- Enforce veto rights on architectural decisions
- Validate delegation authority before spawning sub-agents

## See Also

- [[README|Agent Template]]
- [[coordination|Coordination Protocol]]
