# OVERVIEW — Agent Base Profile

| | |
|---|---|
| **Document** | docs/OVERVIEW.en.md |
| **Version** | 1.0 |
| **Date** | 2026-05-22 |
| **Bilingual Pair** | OVERVIEW.th.md |

> This is the **entry door**. Five minutes to know what this is and what to read next.

---

## What This Is

Agent Base Profile is a **template for creating AI coding agents** designed end-to-end against Buddhist principles.

- **One repo, one agent** — each agent lives in its own repository, cloned from this template
- **Backend-neutral** — runs on Claude, Antigravity, Codex, Kimi
- **Remembers and connects** — accumulates knowledge across sessions
- **Co-operates** — multiple agents in the same repo without collision

---

## Why Buddhist Principles

Not decoration. Buddhist frameworks provide a **deep and complete engineering thinking framework** for the problems AI agents actually face.

| Problem | Buddhist Framework |
|---|---|
| Designing requirements | Magga 8 |
| Architecting | Khandha 5 |
| Solving problems | Ariyasacca 4 |
| Managing state | Tilakkhaṇa |
| Audit logging | Kamma 3 |
| Observability | Satipaṭṭhāna 4 |
| Failure analysis | Paṭiccasamuppāda |
| Lifecycle | Bhāvanā 4 |
| Self-improvement | Paññā 3 |
| Threat modeling | Taṇhā 3 |
| Fleet governance | Aparihāniya-dhamma 7 |
| Error UX | Brahmavihāra 4 |
| Inter-agent trust | Kalyāṇamitta 7 |

See [PHILOSOPHY.en.md](PHILOSOPHY.en.md) for the full mapping.

---

## Getting Started

### I'm an Agent Author (building a new agent)
```bash
bwoc new <agent-name>
cd agents/agent-<agent-name>
# Edit persona/README.md
# Review config.manifest.json
bwoc check .
```
Read next: [HANDBOOK.en.md](HANDBOOK.en.md)

### I'm an Agent Operator (using agents)
Read next: [HANDBOOK.en.md](HANDBOOK.en.md) → [SRS.en.md](SRS.en.md) section 5

### I'm a Platform Maintainer
Read: [GLOSSARY](../../../../docs/en/GLOSSARY.en.md) → [PHILOSOPHY](PHILOSOPHY.en.md) → everything

### I want the philosophy first
Read: [PHILOSOPHY.en.md](PHILOSOPHY.en.md)

---

## Document Map

```
docs/
├── {en,th}/
│   ├── PHILOSOPHY               ← Buddhist foundations (read first)
│   ├── OVERVIEW                 ← this file
│   ├── HANDBOOK                 ← working as an agent
│   ├── PRD                      ← Product (Ariyasacca 4)
│   ├── SRS                      ← Requirements (Magga 8)
│   ├── SELF-IMPROVEMENT         ← Learning (Paññā 3)
│   └── THREAT-MODEL             ← Security (Taṇhā 3)
├── persona-example.{good,bad}.md                ← good/bad persona examples
├── project-example.md · reference-example.md    ← memory file examples
└── task-log.example.jsonl                       ← task-log example

Framework-root docs/ (ARCHITECTURE, FLEET-GOVERNANCE, GLOSSARY) sit outside the template.
```

---

## Reading Paths

### 🟢 Path 1 — Express (30 min)
1. OVERVIEW (here)
2. examples/workflow/incarnation.md
3. examples/workflow/first-task.md

### 🟡 Path 2 — Understanding (2 hours)
1. OVERVIEW
2. PHILOSOPHY (skim groups A–F)
3. PRD
4. SRS
5. ARCHITECTURE

### 🔴 Path 3 — Depth (one day)
Read every file in docs/ + examples/ in order.

---

## Five Principles You Must Know

Out of 22 frameworks, these are the five most commonly applied.

### 1. Yoniso Manasikāra — Verify Before Act
Memory is a past claim; verify against present state before acting.

### 2. Mattaññutā — Right Amount
MEMORY.md ≤ 200 lines to force selection of what matters.

### 3. Anattā — Non-Clinging
Task done → cleanup worktree → delete branch. No clinging.

### 4. Samānattatā — Equal Treatment
All backends equal: every backend file is a symlink to a single AGENTS.md.

### 5. Sīla-sāmaññatā — Communal Convention
All agents under the same rules via conventions.md + neutrality check.

---

## FAQ

**Q: Do I need to be Buddhist or understand Buddhism?**
A: No. Use it as an engineering framework. Pali words are just section names; content is technical.

**Q: Why not just use DDD, Clean Architecture, SOLID?**
A: You still can — they don't conflict. Buddhist frameworks add coverage in state, failure, and lifecycle areas where Western frameworks are thinner.

**Q: This is a lot of documentation. Must I read all of it?**
A: No. Read OVERVIEW + PHILOSOPHY first. The rest is on-demand.

**Q: If I don't like Buddhist framing, can I still use this?**
A: Yes — strip PHILOSOPHY and keep the technical skeleton. You lose the "why" behind decisions.

---

## Current Status

| Area | Status |
|---|---|
| Core docs (PHILOSOPHY, PRD, SRS, ARCH) | ✅ Ready |
| Lifecycle, Observability, Failure, Improvement | ✅ Ready |
| Coordination, Governance, Threat | ✅ Ready |
| Examples | ✅ Ready |
| Reference agents | ⏳ Phase 4 |
| Fleet dashboard | ⏳ Phase 4 |
