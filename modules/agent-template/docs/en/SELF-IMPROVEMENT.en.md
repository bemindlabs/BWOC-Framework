# SELF-IMPROVEMENT — Learning Loop (Structured by Paññā 3)

| | |
|---|---|
| **Document** | docs/SELF-IMPROVEMENT.en.md |
| **Bilingual Pair** | SELF-IMPROVEMENT.th.md |
| **Primary Framework** | Paññā 3 (Three Roots of Wisdom) |
| **Supporting** | Iddhipāda 4 (vīmaṃsā), Bhāvanā 4 |

---

## 0. Principle

In Buddhism, wisdom arises from three sources, not one.

| Type | Meaning | In the System |
|---|---|---|
| Sutamayā paññā | Listening, reading, studying | Study docs, conventions, examples |
| Cintāmayā paññā | Reflection, synthesis | Pattern extraction, decisions |
| Bhāvanāmayā paññā | Practice, experience | Feedback, retrospectives |

> **Rule:** Practical wisdom requires all three. Missing one → shallow or wrong.

---

## 1. Sutamayā — Learn from Study

### 1.1 Inputs
- AGENTS.md (and symlinks)
- conventions/*.md
- docs/* (PHILOSOPHY → ARCHITECTURE)
- examples/*
- Peer skill files (capabilities.md)
- Tier 2 memory (cross-agent insights)

### 1.2 Activities
- **Session start:** load relevant docs before work
- **Pre-task:** read related memories
- **On unknown:** search skill files and knowledge base

### 1.3 Storage
Memory files type: `reference-*`

```markdown
# memories/reference-postgres-naming.md
---
type: reference
source: conventions/database.md#naming
date: 2026-05-22
verifiedAgainst: schema.sql@abc123
---

PostgreSQL naming conventions used here:
- Tables: snake_case plural
- Columns: snake_case
- Indexes: idx_<table>_<columns>
```

### 1.4 Quality Check
- Source traceable? (link / reference present)
- Verification date? (was it ever checked against current code)
- Selective? (not a dump → mattaññutā)

---

## 2. Cintāmayā — Learn from Reflection

### 2.1 Activities
- **Pattern extraction:** after several similar tasks → look for patterns
- **Decision rationale:** before big decisions → write rationale, alternatives
- **Synthesis:** connect multiple sources of suta
- **Mental simulation:** "if I do X, then…" — yoniso manasikāra

### 2.2 Storage
Memory files type: `project-*` or `decision-*`

```markdown
# memories/decision-2026-05-22-caching-strategy.md
---
type: decision
date: 2026-05-22
status: active
references:
  - reference-redis-cluster.md
  - feedback-PROJ-30-cache-thrashing.md
---

## Decision
Use Redis Sentinel instead of Cluster mode.

## Alternatives Considered
- A: Redis Cluster — complexity exceeds current scale
- B: Redis Sentinel — chosen
- C: Memcached — lacks persistence

## Rationale
Per reference-redis-cluster.md + feedback-PROJ-30.
Sentinel matches required scale and availability.

## Revisit If
- Scale exceeds 50k req/s
- Multi-region requirement appears
```

### 2.3 Quality Check
- Alternatives considered?
- Rationale references sources?
- Revisit conditions stated?

### 2.4 Core Reflection — the Like / Wish / Next Step ritual

The practical, repeatable heart of Cintāmayā: a **three-line retrospective** run at the close of every task and every session. It is the smallest reflection that still turns raw experience into knowledge — cheap enough to *always* do (**Mattaññutā**), structured enough to compound.

Grounded in **Paccavekkhaṇa** (reflective reviewing) — the Buddha's instruction to Rāhula (MN 61) to review each action *before, during, and after*. Core Reflection is the disciplined "after."

**The three prongs:**

| Prong | Asks | Produces | Feeds |
|---|---|---|---|
| **Like** | What worked — worth keeping / repeating? | A reinforced pattern | Suta (a `reference-*`) or a capability |
| **Wish** | What fell short — the gap between expected and actual? | A *named gap* (not a complaint) | Cintā (a `decision-*`) or a `feedback-*` |
| **Next step** | The single most useful thing to change next time? | One concrete, owned commitment (**Adhiṭṭhāna**) | Bhāvanā (the next task's hypothesis) |

**Rules that keep it honest:**
- **One line each.** If a prong needs a paragraph, it is a `decision-*`, not a reflection line — promote it.
- **Wish names a gap, not a grievance.** "Wish I'd checked the migration on a copy first" — actionable. Not "the schema was bad."
- **Exactly one Next step.** Many next-steps = none. Pick the highest-leverage one; the rest wait.
- **Next step is owned + concrete** — it has a verb and a subject you control.

**Template** (append to the task's `feedback-*` or the session note):

```markdown
## Core Reflection
- Like: <what worked, keep it>
- Wish: <the gap — expected vs actual, actionable>
- Next step: <the one concrete change, owned>
```

**Example:**

```markdown
## Core Reflection
- Like: Wrote the rollback script before the migration — made the failed run a non-event.
- Wish: Ran the migration against prod-shaped data too late; the row-count assumption only broke at scale.
- Next step: Add a "test against a prod-sized copy" step to the migration checklist before any ALTER.
```

**How it threads every stage** — Core Reflection is the connective tissue of the Wisdom Loop (§4). Run it at each stage boundary and it converts that stage's output into the next stage's input:

| Stage | Core Reflection asks there | The Next step becomes |
|---|---|---|
| After **Suta** (study) | Like: which source actually helped? · Wish: what was missing or stale? | A better source to load next time (or a verification to run) |
| After **Cintā** (a decision) | Like: which alternative held up? · Wish: which assumption was weakest? | A `Revisit If` condition on the `decision-*` |
| After **Bhāvanā** (practice) | Like: what worked in execution? · Wish: the expected-vs-actual gap? | The hypothesis for the next attempt (→ new suta after curation) |

**Escalation:** when the *same* Wish recurs 3+ times, its Next step has failed to land — push it up the Curation Pipeline (§5): from a personal reflection line → a `decision-*` → a shared capability → a convention. A repeating Wish is the fleet telling you a habit, not a task, needs to change.

---

## 3. Bhāvanāmayā — Learn from Practice

### 3.1 Activities
- **Post-task feedback:** record actual vs expected
- **Post-mortem:** paṭiccasamuppāda chain analysis
- **Retrospective:** weekly review
- **A/B observation:** patterns under real use

### 3.2 Storage
Memory files type: `feedback-*`

```markdown
# memories/feedback-PROJ-42-schema-migration.md
---
type: feedback
date: 2026-05-22
task: PROJ-42
outcome: success-with-issues
---

## Expected
Migration < 30 min, no downtime

## Actual
- 47 min (50% over estimate)
- Brief 2s lock on users table

## Why (short paṭiccasamuppāda)
- Avijjā: didn't know real users-table size (only count, not indexes)
- Saṅkhāra: estimate wrong → didn't schedule for low-traffic window

## Lessons
- Add pre-migration size check to skill file
- Update reference-schema-migration.md

## Convention Impact
Yes → submit CCP
```

### 3.3 Quality Check
- Expected vs actual explicit?
- Causal chain (not just "what happened")?
- Action items concrete?

---

## 4. The Wisdom Loop

```
       ┌─────────────────────────────┐
       │  Suta (Study)                │
       │  reference-*.md             │
       └──────────┬──────────────────┘
                  │ informs
                  ▼
       ┌─────────────────────────────┐
       │  Cintā (Reflect)             │
       │  decision-*.md, project-*.md│
       └──────────┬──────────────────┘
                  │ becomes hypothesis
                  ▼
       ┌─────────────────────────────┐
       │  Bhāvanā (Practice)          │
       │  feedback-*.md              │
       └──────────┬──────────────────┘
                  │
                  │ feeds back as new suta
                  │ (after curation)
                  ▼
           Updated suta (via Tier 2 mining)
```

**The ritual that drives the loop is Core Reflection (§2.4).** Run the Like / Wish / Next Step retro at each arrow: *Like* reinforces the stage you just left, *Wish* names the gap that moves you to the next stage, and *Next step* is the hypothesis you carry into it. Without the ritual the loop is a diagram; with it, every task advances the wheel one turn.

---

## 5. Curation Pipeline

Not every feedback becomes knowledge — it must be curated.

### Level 1 — Personal (Tier 1)
- Agent keeps it under memories/
- Verify in the next session

### Level 2 — Pattern Detected
- After the same pattern 3+ times → mine to decision-*.md
- Begin sharing in capabilities.md

### Level 3 — Cross-Agent (Tier 2)
- When the pattern benefits more than one agent
- Mine to Tier 2 memory
- Add to skill files

### Level 4 — Convention
- When the pattern is fleet-wide best practice
- Submit via CCP (FLEET-GOVERNANCE §3)

---

## 6. Self-Improvement Metrics

### 6.1 Suta Metrics
- Source diversity: number of sources cited per decision
- Verification rate: % references checked recently
- Reading depth: time spent loading docs

### 6.2 Cintā Metrics
- Decision quality: alternatives per decision
- Synthesis count: cross-references in memory files
- Revisit accuracy: when revisited, were decisions revised?

### 6.3 Bhāvanā Metrics
- Post-mortem completion rate
- Action item closure rate
- Pattern detection latency: how many occurrences before pattern is named

### 6.4 Combined: Vīmaṃsā (Iddhipāda 4)
- Improvement velocity: feedback → action time
- Knowledge half-life: how long memory stays valid before needing re-verification

---

## 7. Anti-Patterns

| Pattern | Missing |
|---|---|
| Memorizing docs without testing | Bhāvanā |
| Patching without analysis | Cintā |
| Reinventing patterns | Suta |
| Endless reflection, no action | Bhāvanā |
| Cargo-culting from other agents | Cintā + Bhāvanā |

---

## 8. Self-Improvement Triggers

### Trigger 1 — Task Failure
→ Post-mortem (bhāvanā) + check existing suta

### Trigger 2 — Repeated Same Issue
→ Pattern extraction (cintā) + mine to Tier 2

### Trigger 3 — Promotion Eligibility
→ Demonstrate all three paññā (Ariya-dhana L5/L6)

### Trigger 4 — Convention Update
→ Re-read affected suta, update references

### Trigger 5 — Fleet Sync
→ Share insights (sāraṇīyadhamma + cāga)

---

## 9. Relationship to Other Documents

| Document | Connection |
|---|---|
| PHILOSOPHY | Paññā 3 (DP-13), Iddhipāda vīmaṃsā |
| LIFECYCLE | L4 → L5 requires all three paññā |
| FAILURE-MODES | Post-mortem feeds bhāvanā |
| OBSERVABILITY | Rule application logs = suta source |
| FLEET-GOVERNANCE | Mined patterns → CCP |
| SRS | FR-8 (Sammā-samādhi) memory system |
