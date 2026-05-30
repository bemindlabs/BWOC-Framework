# SRS — Software Requirements Specification

## Agent Base Profile (Structured by Magga 8 — The Noble Eightfold Path)

| | |
|---|---|
| **Document** | SRS.en.md |
| **Version** | 2.0 |
| **Date** | 2026-05-22 |
| **Bilingual Pair** | SRS.th.md |
| **Philosophy Reference** | PHILOSOPHY.en.md |
| **PRD Reference** | PRD.en.md |

> **Document spine:** Functional Requirements organized into eight categories by the Noble Eightfold Path.
> Cross-cutting principles: *yoniso manasikāra*, *tilakkhaṇa*, *mattaññutā*.

---

## 0. Introduction

### 0.1 Purpose
This document specifies the software requirements — functional and non-functional — of the Agent Base Profile, along with its interfaces and data contracts.

### 0.2 Scope
The system under specification is a **template repository**, not a runtime.

### 0.3 Notation
- **Priority:** M = Must, S = Should, C = Could
- **Verify:** T = Test, I = Inspection, D = Demo, A = Analysis
- **Requirement ID:** `FR-{Magga}.{seq}` — e.g., `FR-7.1` is the first requirement under *Sammā-sati*

---

## 1. Functional Requirements (Organized by Magga 8)

### Pillar 1 — Sammā-diṭṭhi (Right View): Persona & Identity

| ID | P | Requirement | V |
|---|---|---|---|
| FR-1.1 | M | `persona/README.md` SHALL define the agent's identity, role, principles, and constraints | I |
| FR-1.2 | M | The persona SHALL declare its scope of capability (*attaññutā*) | I |
| FR-1.3 | M | The persona SHALL state what the agent **does not do** (*mattaññutā* boundaries) | I |
| FR-1.4 | S | The persona SHOULD reference PHILOSOPHY.md to show its thinking basis | I |
| FR-1.5 | M | `AGENTS.md` SHALL be the single source of truth | I |

### Pillar 2 — Sammā-saṅkappa (Right Intention): Goal Setting

| ID | P | Requirement | V |
|---|---|---|---|
| FR-2.1 | M | Every task SHALL begin with the Four-Noble-Truths cycle (dukkha → samudaya → nirodha → magga) | A |
| FR-2.2 | M | Tasks SHALL carry a `taskId` and a measurable `goal` | T |
| FR-2.3 | M | Tasks SHALL be tracked in `task-log.jsonl` (one JSON per line) | T |
| FR-2.4 | M | The status field SHALL be one of: `pending`, `in_progress`, `blocked`, `completed`, `failed` | T |
| FR-2.5 | M | `task-log.jsonl` SHALL be append-only | A |
| FR-2.6 | S | Tasks SHOULD declare scope boundaries (*mattaññutā*) before starting | I |

### Pillar 3 — Sammā-vācā (Right Speech): Inter-Agent Communication

| ID | P | Requirement | V |
|---|---|---|---|
| FR-3.1 | M | `interconnect/capabilities.md` SHALL declare skills in a machine-readable form | T |
| FR-3.2 | S | `interconnect/coordination.md` SHALL define phases, messaging, and consensus | I |
| FR-3.3 | S | Inter-agent messages SHALL be concise and context-complete (*piyavācā*) | A |
| FR-3.4 | M | Error messages SHALL state the root cause and the remedy, not merely that something failed | A |
| FR-3.5 | C | Agents COULD publish their capabilities to a shared registry | D |
| FR-3.6 | M | All documents SHALL be bilingual (Thai + English) | I |

### Pillar 4 — Sammā-kammanta (Right Action): Worktree & Commit Discipline

| ID | P | Requirement | V |
|---|---|---|---|
| FR-4.1 | M | Each task SHALL execute in its own worktree at `{{worktreeBase}}/{{taskId}}` | T |
| FR-4.2 | M | Agents SHALL NOT share a working directory with another agent (*anattā*) | A |
| FR-4.3 | M | Agents SHALL NOT use `git stash` | A |
| FR-4.4 | M | Agents SHALL NOT switch branches in place; they SHALL use worktrees | A |
| FR-4.5 | M | Commits SHALL be scoped to files the agent created or modified in this task | A |
| FR-4.6 | M | The history strategy SHALL be rebase, not merge | A |
| FR-4.7 | M | Branch names SHALL be `<type>/{{taskId}}` where `<type>` ∈ {`feat fix docs refactor test chore perf style ci`}; the multi-agent collision guard prefixes `agent/{{agentId}}/`. There SHALL be no `release/*` or `hotfix/*` branches — version tags are cut directly on `main` (trunk-based) | T |
| FR-4.8 | M | After merge, the worktree SHALL be removed and the local branch deleted (*anattā* — release) | A |

### Pillar 5 — Sammā-ājīva (Right Livelihood): Trust & Neutrality

| ID | P | Requirement | V |
|---|---|---|---|
| FR-5.1 | M | `AGENTS.md` SHALL be a regular file; `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md` SHALL be symlinks pointing to `AGENTS.md` | T |
| FR-5.2 | M | No instruction file SHALL contain backend-specific content contradicting AGENTS.md | A |
| FR-5.3 | M | `check-agent-neutrality.sh` SHALL fail if any symlink is broken or replaced by a regular file | T |
| FR-5.4 | M | `trust-model.md` SHALL document the security posture for cloning external agents | I |
| FR-5.5 | S | Hooks in `.claude/settings.json` SHOULD restrict destructive actions | T |
| FR-5.6 | M | No secrets SHALL be committed to memory files (*samānattatā* + discipline) | T |
| FR-5.7 | M | New backends SHALL be addable via new symlinks, with no code change | I |

### Pillar 6 — Sammā-vāyāma (Right Effort): Verification Gates

Mapped onto the Four Padhāna.

| ID | P | Requirement | Padhāna | V |
|---|---|---|---|---|
| FR-6.1 | M | The agent SHALL run `{{lintCmd}}` before declaring work done | Saṃvara | T |
| FR-6.2 | M | The agent SHALL run `{{formatCmd}}` on every code change | Pahāna | T |
| FR-6.3 | M | The agent SHALL run `{{testCmd}}` on every logic change | Bhāvanā | T |
| FR-6.4 | M | The agent SHALL run regression tests to preserve existing features | Anurakkhanā | T |
| FR-6.5 | M | The agent SHALL run `{{buildCmd}}` before pushing build-affecting changes | Saṃvara | T |
| FR-6.6 | M | UI changes SHALL be verified against a running dev server | Bhāvanā | D |
| FR-6.7 | M | Work SHALL NOT be declared complete until all applicable gates pass | (all) | A |

### Pillar 7 — Sammā-sati (Right Mindfulness): Memory System

#### 7.1 Tier 1 — File-Based Memory

| ID | P | Requirement | V |
|---|---|---|---|
| FR-7.1 | M | Memory files SHALL live under `{{memoryPath}}` (default `memories/`) | I |
| FR-7.2 | M | Each memory file SHALL have YAML front-matter: `name`, `description`, `type`, `created`, `updated` | T |
| FR-7.3 | M | `type` SHALL be one of: `user`, `feedback`, `project`, `reference` | T |
| FR-7.4 | M | `feedback` and `project` memories SHALL include **Why** and **How to apply** sections | I |
| FR-7.5 | M | The agent SHALL convert relative dates ("Thursday") to absolute ISO dates | T |
| FR-7.6 | M | `MEMORY.md` (index) SHALL NOT exceed 200 lines (*mattaññutā*) | T |
| FR-7.7 | M | The agent SHALL verify memory claims against current code before acting (*yoniso manasikāra*) | A |
| FR-7.8 | S | The agent SHOULD save memories from both failures AND successes | A |
| FR-7.9 | S | The agent SHOULD NOT save anything derivable from code, git history, or AGENTS.md (*mattaññutā*) | A |
| FR-7.10 | M | Memory SHALL be pruned per policy (*aniccaṃ*) | A |

#### 7.2 Tier 2 — Deep Memory Backend (Optional)

| ID | P | Requirement | V |
|---|---|---|---|
| FR-7.11 | S | The system SHOULD expose a `{{deepMemoryCmd}}` placeholder in config | I |
| FR-7.12 | S | The deep memory backend SHALL support verbs: `wake-up`, `search <query>`, `mine <path>` | T |
| FR-7.13 | C | The agent COULD invoke `wake-up` at session start | D |
| FR-7.14 | C | The agent COULD invoke `mine` at session end | D |
| FR-7.15 | M | Tier 2 SHALL be optional; its absence MUST NOT break the agent | T |

#### 7.3 Session Lifecycle

| ID | P | Requirement | V |
|---|---|---|---|
| FR-7.16 | M | Session start SHALL load `MEMORY.md`, relevant memories, and `task-log.jsonl` | T |
| FR-7.17 | M | Session start SHALL verify memory claims against current code | A |
| FR-7.18 | M | Session end SHALL update `task-log.jsonl` | T |
| FR-7.19 | M | Session end SHALL persist new discoveries as Tier 1 memories | A |
| FR-7.20 | S | Session end SHOULD remove stale memories (*aniccaṃ*) | A |

### Pillar 8 — Sammā-samādhi (Right Concentration): Focus & Stability

| ID | P | Requirement | V |
|---|---|---|---|
| FR-8.1 | M | `config.manifest.json` SHALL declare every required placeholder | T |
| FR-8.2 | M | Validation SHALL fail if any required placeholder is unset | T |
| FR-8.3 | M | The default config SHALL include: `agentId`, `model`, `fallbackModel`, `maxConcurrentTasks`, `worktreeIsolation`, `worktreeBase`, `memory.*` | I |
| FR-8.4 | M | `scripts/incarnate.sh <agent-name>` SHALL clone the template into a new agent | T |
| FR-8.5 | M | `scripts/check-agent-neutrality.sh` SHALL validate structural conformance | T |
| FR-8.6 | M | Scripts SHALL exit non-zero on failure | T |
| FR-8.7 | S | Scripts SHOULD print a human-readable summary | D |
| FR-8.8 | S | `.claude/commands/new-agent` SHOULD invoke `incarnate.sh` from inside Claude Code | T |

---

## 2. Cross-Cutting Principles

### 2.1 Yoniso Manasikāra — Verify Before Act
Every FR that reads from memory must verify against current state — covered explicitly by FR-7.7 and FR-7.17.

### 2.2 Tilakkhaṇa — State Philosophy

| Mark | Impact on FRs |
|---|---|
| Aniccaṃ | FR-7.10 (prune), FR-4.8 (cleanup), FR-7.2 (timestamps) |
| Dukkhaṃ | FR-4.8 (no stale branch), FR-7.20 (no stale memory) |
| Anattā | FR-4.2 (no shared dir), FR-4.3 (no stash), FR-4.8 (release) |

### 2.3 Mattaññutā — Scope Discipline
- No work outside the task's scope (FR-4.5)
- No saving of derivable information (FR-7.9)
- No debugging effects outside the task's boundary (FR-1.3)

---

## 3. Non-Functional Requirements

### 3.1 Portability (Samānattatā)

| ID | Requirement |
|---|---|
| NFR-1.1 | The system SHALL run on Linux, macOS, and Windows (via WSL or Git Bash) |
| NFR-1.2 | LLM-agnostic — identical behavior across all backends per FR-5.1 |

### 3.2 Performance (Mattaññutā — Right Amount)

| ID | Requirement |
|---|---|
| NFR-2.1 | `incarnate.sh` SHALL complete in ≤ 5 s on a developer laptop |
| NFR-2.2 | `check-agent-neutrality.sh` SHALL complete in ≤ 2 s |
| NFR-2.3 | Session-start memory load SHALL complete in ≤ 1 s when MEMORY.md ≤ 200 lines |

### 3.3 Reliability (Sammā-samādhi — Steadiness)

| ID | Requirement |
|---|---|
| NFR-3.1 | Scripts SHALL be idempotent unless explicitly destructive |
| NFR-3.2 | A failed incarnation SHALL leave no partial directory |
| NFR-3.3 | Worktree-creation failure SHALL roll back cleanly |

### 3.4 Maintainability (Sīla-sāmaññatā)

| ID | Requirement |
|---|---|
| NFR-4.1 | All cross-backend instruction files SHALL be symlinks (no duplication) |
| NFR-4.2 | Conventions SHALL be documented in `conventions.md` |
| NFR-4.3 | Every requirement SHALL be traceable via the matrix in Appendix C |

### 3.5 Security (Sammā-ājīva)

| ID | Requirement |
|---|---|
| NFR-5.1 | No secrets SHALL be committed to memory files |
| NFR-5.2 | The trust model SHALL be documented and enforceable via hooks |
| NFR-5.3 | Hooks SHALL deny `rm -rf` of repository root |

### 3.6 Usability (Saṅgahavatthu 4)

| ID | Requirement |
|---|---|
| NFR-6.1 | A new agent SHALL be ready to commit within 30 minutes |
| NFR-6.2 | Error messages SHALL name the offending placeholder or symlink |

### 3.7 Scalability

| ID | Requirement |
|---|---|
| NFR-7.1 | The system SHALL support `maxConcurrentTasks` ≥ 3 by default |
| NFR-7.2 | Worktree isolation SHALL allow N concurrent tasks bounded only by host disk and CPU |

### 3.8 Auditability (Vīmaṃsā)

| ID | Requirement |
|---|---|
| NFR-8.1 | `task-log.jsonl` SHALL provide an append-only audit trail |
| NFR-8.2 | Every memory file SHALL carry `created` and `updated` ISO timestamps |

---

## 4. External Interfaces

### 4.1 LLM CLI Interfaces

| Backend | Entry File | Mechanism |
|---|---|---|
| Claude | `CLAUDE.md` → `AGENTS.md` | Symlink |
| Antigravity | `AGY.md` → `AGENTS.md` | Symlink |
| Codex | `CODEX.md` → `AGENTS.md` | Symlink |
| Kimi | `KIMI.md` → `AGENTS.md` | Symlink |
| Generic | `AGENTS.md` | Direct |

### 4.2 Deep Memory Backend Interface (Tier 2)

```
{{deepMemoryCmd}} wake-up                     # emit session-start context to stdout
{{deepMemoryCmd}} search "<query>"            # emit ranked results to stdout
{{deepMemoryCmd}} mine <path> --mode <mode>   # persist learnings
```

Exit codes: 0 success, non-zero failure.

### 4.3 Project Submodule Interface
Project repos are mounted via `git submodule add <url> projects/<name>`.

---

## 5. Data Schemas

### 5.1 Memory File Schema

```yaml
---
name: <descriptive name>             # required
description: <one-line hook>         # required
type: user|feedback|project|reference  # required
created: <ISO 8601>                  # required
updated: <ISO 8601>                  # required
---

<content body>

**Why:** <motivation>                # required for feedback, project
**How to apply:** <when/where>       # required for feedback, project
```

### 5.2 Task Log Record Schema

```json
{
  "taskId":        "string",         // required, unique
  "moduleName":    "string",         // required
  "branchName":    "string",         // required, matches FR-4.7
  "worktreePath":  "string",         // required, absolute
  "status":        "string",         // required, FR-2.4 enum
  "startedAt":     "ISO-8601",       // required
  "lastAction":    "string",         // required
  "completedAt":   "ISO-8601",       // optional
  "blockedReason": "string"          // optional, required if status=blocked
}
```

### 5.3 Config Manifest Schema

```json
{
  "agentId":            "agent-{{name}}",
  "model":              "{{primaryModel}}",
  "fallbackModel":      "{{fallbackModel}}",
  "maxConcurrentTasks": 3,
  "worktreeIsolation":  true,
  "worktreeBase":       "/tmp",
  "memory": {
    "fileBasedPath":      "{{memoryPath}}",
    "deepMemoryCmd":      "{{deepMemoryCmd}}",
    "wakeUpOnStart":      true,
    "maxMemoryIndexLines": 200
  }
}
```

### 5.4 Required Placeholders

| Placeholder | Type | Required | Resolved By |
|---|---|---|---|
| `{{name}}` | string | yes | `incarnate.sh` argument |
| `{{agentId}}` | string | yes | derived from `{{name}}` |
| `{{primaryModel}}` | string | yes | user edit |
| `{{fallbackModel}}` | string | no | user edit |
| `{{memoryPath}}` | path | yes | default `memories/` |
| `{{deepMemoryCmd}}` | string | no | user edit |
| `{{lintCmd}}` | string | yes | user edit |
| `{{testCmd}}` | string | yes | user edit |
| `{{buildCmd}}` | string | yes | user edit |
| `{{formatCmd}}` | string | yes | user edit |
| `{{worktreeBase}}` | path | no | default `/tmp` |
| `{{taskId}}` | string | runtime | task assignment |

---

## 6. Verification & Validation

### 6.1 Automated Checks (`check-agent-neutrality.sh`)
1. `AGENTS.md` exists and is a regular file
2. `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md` are symlinks to `AGENTS.md`
3. All required placeholders are substituted
4. `config.manifest.json` parses as JSON
5. `task-log.jsonl` is valid JSONL
6. Every memory file has valid front-matter and a required `type`
7. `MEMORY.md` ≤ 200 lines
8. `AGENTS.md` contains no backend-specific lock-in

### 6.2 Acceptance Criteria (by Magga)

| Magga | Acceptance |
|---|---|
| Sammā-diṭṭhi | Persona passes the checklist; vision is clear |
| Sammā-saṅkappa | All tasks carry a `taskId` and goal |
| Sammā-vācā | Two agents complete a consensus exchange |
| Sammā-kammanta | Three concurrent agents, zero collisions |
| Sammā-ājīva | Six backends, equivalent behavior |
| Sammā-vāyāma | 100% gates pass on merged PRs |
| Sammā-sati | ≥ 95% on the prior-decision test |
| Sammā-samādhi | Incarnation ≤ 5 s; check ≤ 2 s |

---

## Appendix A — Branch Naming Grammar

```
branch       ::= category "/" identifier
category     ::= "feature" | "fix" | "refactor" | "release" | "hotfix"
               | "agent/" agent-id
agent-id     ::= [a-z0-9-]+
identifier   ::= task-id | version
task-id      ::= [A-Z]+ "-" [0-9]+
version      ::= "v" [0-9]+ "." [0-9]+ "." [0-9]+
```

## Appendix B — Worktree State Machine

```
[*] --> Create        : task assigned (sammā-saṅkappa)
Create --> Work       : worktree ready
Work --> Verify       : code complete
Verify --> Fix        : gates fail (sammā-vāyāma)
Fix --> Verify        : retry
Verify --> Land       : gates pass
Land --> Cleanup      : merged
Cleanup --> [*]       : (anattā — release)
```

## Appendix C — Traceability Matrix

| PRD Section | SRS Requirements |
|---|---|
| Part 4 — Magga / Sammā-diṭṭhi | FR-1.1–1.5 |
| Part 4 — Magga / Sammā-saṅkappa | FR-2.1–2.6 |
| Part 4 — Magga / Sammā-vācā | FR-3.1–3.6 |
| Part 4 — Magga / Sammā-kammanta | FR-4.1–4.8 |
| Part 4 — Magga / Sammā-ājīva | FR-5.1–5.7 |
| Part 4 — Magga / Sammā-vāyāma | FR-6.1–6.7 |
| Part 4 — Magga / Sammā-sati | FR-7.1–7.20 |
| Part 4 — Magga / Sammā-samādhi | FR-8.1–8.8 |
| Part 7 — Iddhipāda | NFR-1 through NFR-8 |
| Part 8 — Tilakkhaṇa | Cross-cutting §2.2 |
| Part 9 — Out of scope | Cross-cutting §2.3 (Mattaññutā) |

---

## Appendix — Changelog

### v2.0 (2026-05-22)
- **Fixed forced metaphors:** Replaced `acinteyya` → `mattaññutā` in cases meaning "knowing moderation of work scope". Acinteyya is reserved for its original four cases (Buddha-visaya, Jhāna-visaya, Kamma-vipāka, Loka-cintā).
- **Added companion documents:**
  - `FAILURE-MODES.md` (Paṭiccasamuppāda) — failure analysis
  - `LIFECYCLE.md` (Bhāvanā 4 + Ariya-dhana 7) — agent lifecycle
  - `OBSERVABILITY.md` (Satipaṭṭhāna 4 + Kamma 3) — monitoring + audit
  - `COORDINATION-PROTOCOL.md` (Kalyāṇamitta 7 + Sāraṇīyadhamma 6) — inter-agent
  - `FLEET-GOVERNANCE.md` (Aparihāniya-dhamma 7) — org-level governance
  - `SELF-IMPROVEMENT.md` (Paññā 3) — learning loop
  - `THREAT-MODEL.md` (Taṇhā 3 + Sīla 5) — security
  - `ANTIPATTERNS.md` (Micchā- per Magga 8) — wrong-path catalog
  - `GLOSSARY.md` — Pali + technical terms reference
  - `OVERVIEW.md` — entry-point document
- **Extended PHILOSOPHY.md** to cover 22 frameworks (was 13) across six groups.

### v1.0 (2026-05-22)
- Initial four documents (PHILOSOPHY, PRD, SRS, ARCHITECTURE) bilingual.
