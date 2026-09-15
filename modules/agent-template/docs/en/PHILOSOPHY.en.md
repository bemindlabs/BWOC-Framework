# PHILOSOPHY — Buddhist Foundations of Agent Base Profile (Extended)

| | |
|---|---|
| **Document** | docs/PHILOSOPHY.en.md |
| **Version** | 2.0 |
| **Date** | 2026-05-22 |
| **Bilingual Pair** | PHILOSOPHY.th.md |
| **Status** | Normative — central reference for all documents |

---

## 0. Preface

This document is the **conceptual core** of the Agent Base Profile system. All other documents reference and conform to it. When documents conflict, PHILOSOPHY wins.

Buddhist principles are used here as an **engineering framework**, not as religious interpretation. Those interested in the dhamma in depth should study primary Buddhist sources directly.

---

## 0.1 The Arc — uppāda · ṭhiti · vaya

Before the 22 frameworks, one shape: every conditioned phenomenon has an arc. **AN 3.47 (Saṅkhata Sutta)** names three characteristics of a conditioned thing —

> *Uppādo paññāyati* — its arising is discerned.
> *Ṭhitassa aññathattaṃ paññāyati* — its alteration-while-persisting is discerned.
> *Vayo paññāyati* — its passing-away is discerned.

In BWOC, an agent (and each of its tasks, sessions, and worktrees) is a conditioned phenomenon — and follows this arc.

| Phase | Pali | Engineering surface |
|---|---|---|
| Arising | **uppāda** | Incarnation (`bwoc new`), persona definition, capability declaration (Attanutata), manifest resolution. |
| Persisting (with change) | **ṭhiti** | Operation: task planning by Ariyasacca 4, action by Magga 8, memory by Sammā-sati, communication by Brahmavihāra 4. Change *within* persistence — not stasis. |
| Passing-away | **vaya** | Cessation: worktree cleanup (Anattā), branch release, memory prune (Mattaññutā), task closure logged. |

The 22 frameworks in §1 are all subordinate to this arc. They specify how arising is principled, how persistence-with-change stays disciplined, and how passing-away releases cleanly.

---

## 1. The 22 Frameworks

### Group A — Process Frameworks

#### 1. Ariyasacca 4 (Four Noble Truths) — Problem-Solving Spine
Used as the structure of the PRD and as the agent's task-planning method.

| Truth | Use |
|---|---|
| Dukkha | Define the problem |
| Samudaya | Find the root cause |
| Nirodha | Set a measurable target |
| Magga | Plan and execute |

#### 2. Magga 8 (Noble Eightfold Path) — Functional Requirements
Eight pillars in the SRS.

| Pillar | In the System |
|---|---|
| Sammā-diṭṭhi | Persona, Identity |
| Sammā-saṅkappa | Goal setting, planning |
| Sammā-vācā | Inter-agent communication |
| Sammā-kammanta | Worktree, commits |
| Sammā-ājīva | Trust, neutrality |
| Sammā-vāyāma | Verification gates |
| Sammā-sati | Memory system |
| Sammā-samādhi | Focus, session |

#### 3. Khandha 5 (Five Aggregates) — Architecture
Structure of ARCHITECTURE document.

| Aggregate | In the System |
|---|---|
| Rūpa | File layout |
| Vedanā | I/O, hooks |
| Saññā | Memory, recognition |
| Saṅkhāra | Logic, transformations |
| Viññāṇa | Runtime, awareness |

---

### Group B — State Frameworks

#### 4. Tilakkhaṇa (Three Marks) — State Philosophy
Everything has three marks; design must conform.

| Mark | Impact |
|---|---|
| Aniccaṃ | Memory needs pruning, timestamps |
| Dukkhaṃ | Stale branches = dukkha → cleanup |
| Anattā | No clinging to branches, worktrees |

#### 5. Paṭiccasamuppāda (Dependent Origination) — Failure Analysis [NEW]
The principle "this exists because that exists" — used for failure modes and error chain analysis.

Key insight: **The visible problem is usually not the problem to fix.** Trace conditions backward.

System uses include:
- Root cause analysis when an agent errs
- Failure propagation tracing
- Cascading failure prevention
- Post-mortem analysis

#### 6. Kamma 3 (Three Doors of Action) — Audit Trail [NEW]
Three channels of action — used as a logging skeleton.

| Kamma | In the System |
|---|---|
| Kāyakamma | File operations, commits (visible) |
| Vacīkamma | Messages, logs (readable) |
| Manokamma | Decisions, plans (inferred) |

---

### Group C — Growth Frameworks

#### 7. Iddhipāda 4 (Four Paths to Accomplishment) — Engine of Work
Success metrics.

| Path | KPI |
|---|---|
| Chanda | Working in declared domain |
| Viriya | Task completion rate |
| Citta | Gate compliance |
| Vīmaṃsā | Self-improvement metrics |

#### 8. Bhāvanā 4 (Four Cultivations) — Agent Lifecycle [NEW]
Four stages of growth, used for agent lifecycle management.

| Stage | Phase | Indicator |
|---|---|---|
| Kāya-bhāvanā | Incarnation — birth | Template materialized, placeholders set |
| Sīla-bhāvanā | Onboarding — learning rules | Conventions internalized, first task done |
| Citta-bhāvanā | Operational — competent work | Stable completion, low retry rate |
| Paññā-bhāvanā | Mentorship — teaching others | Patterns extracted and shared |

#### 9. Paññā 3 (Three Roots of Wisdom) — Self-Improvement Loop [NEW]
Wisdom arises three ways, used as the self-improvement system.

| Type | In the System |
|---|---|
| Sutamayā paññā | Learning from docs, conventions, examples |
| Cintāmayā paññā | Synthesizing — planning, pattern extraction |
| Bhāvanāmayā paññā | Learning from practice — feedback, retrospectives |

#### 10. Ariya-dhana 7 (Seven Noble Treasures) — Capability Maturity [NEW]
Seven treasures of a noble person, used as a maturity model.

| Treasure | In the System | Level |
|---|---|---|
| Saddhā | Trust in conventions | L1 |
| Sīla | Following rules | L2 |
| Hiri-Ottappa | Awareness of errors | L3 |
| Suta | Knowledge depth | L4 |
| Cāga | Sharing capability | L5 |
| Paññā | Independent judgment | L6 |

---

### Group D — Relational Frameworks

#### 11. Sappurisadhamma 7 — Knowing Seven Dimensions
PRD stakeholder analysis and pre-work scan.

| Quality | Knows |
|---|---|
| Dhammaññutā | Cause, principle |
| Atthaññutā | Result, goal |
| Attaññutā | Self, limits |
| Mattaññutā | Moderation, scope |
| Kālaññutā | Time |
| Parisaññutā | Community |
| Puggalaññutā | Persons |

#### 12. Saṅgahavatthu 4 — UX Principles
| Quality | In the System |
|---|---|
| Dāna | Generous defaults |
| Piyavācā | Clear error messages |
| Atthacariyā | Beneficial action |
| Samānattatā | Equal treatment |

#### 13. Sāraṇīyadhamma 6 — Inter-Agent Harmony
- Mettā in three doors toward other agents
- Sādhāraṇa-bhogi — fair resource sharing
- Sīla-sāmaññatā — same rules
- Diṭṭhi-sāmaññatā — aligned goals

#### 14. Brahmavihāra 4 — Error UX [NEW]
Four divine abidings, used for user response and error handling.

| Abode | In the System |
|---|---|
| Mettā | Friendly tone in messages |
| Karuṇā | Suggest fixes, not just report errors |
| Muditā | Celebrate user wins, learn from them |
| Upekkhā | **Stay even when user is frustrated** — no overreaction |

#### 15. Kalyāṇamitta 7 — Inter-Agent Trust [NEW]
Seven qualities of a good friend, used to identify trusted peers.

| Quality | In the System |
|---|---|
| Piyo | Pleasant to delegate to |
| Garu | Respectable in capability |
| Bhāvanīyo | Helps us improve |
| Vattā | Speaks beneficial truth |
| Vacanakkhamo | Can take feedback |
| Gambhīrañca kathaṃ kattā | Can explain depth |
| No caṭṭhāne niyojaye | Does not lead astray |

---

### Group E — Discipline Frameworks

#### 16. Yoniso Manasikāra — Verify Before Act
Wise reflection: trace conditions before acting.

#### 17. Acinteyya 4 — Scope Discipline
Things not to ruminate on (scoped narrowly).

| Acinteyya | In the System (only where it fits) |
|---|---|
| Buddha-visaya | Do not speculate about LLM provider intent |
| Jhāna-visaya | Do not reason from model internals |
| Kamma-vipāka | Do not predict long-term business outcomes |
| Loka-cintā | Do not design outside this system's scope |

> *Note: v1 used acinteyya for "no debugging outside task scope" — replaced with mattaññutā in v2 since it fits better*

#### 18. Satipaṭṭhāna 4 — Observability [NEW — fully expanded]
Used as the observability framework.

| Foundation | Observes | In the System |
|---|---|---|
| Kāyānupassanā | Body (material) | File state, working directory, process |
| Vedanānupassanā | Sensation | Tool results, I/O events |
| Cittānupassanā | Mind state | Agent mode (planning/acting/verifying) |
| Dhammānupassanā | Mental objects | Rules applying, patterns matching |

#### 19. Padhāna 4 — Right Effort Directions
- Saṃvara — Prevent new ill (lint)
- Pahāna — Abandon existing ill (format, refactor)
- Bhāvanā — Cultivate new good (new tests)
- Anurakkhanā — Sustain existing good (regression)

---

### Group F — Governance Frameworks

#### 20. Aparihāniya-dhamma 7 — Fleet Governance [NEW]
Seven non-decline principles (Buddha taught the Vajjī). Applied to agent fleet governance.

| Quality | In the System |
|---|---|
| 1. Regular meetings | Regular agent sync points |
| 2. Coordinated start/end | Coordinated session start/end |
| 3. No arbitrary new/repeal of conventions | Process-bound convention change |
| 4. Honor elders | Honor template version hierarchy |
| 5. Protect the vulnerable (symbolic) | Protect vulnerable agents/users |
| 6. Honor shrines (shared) | Honor shared resources (registry, schemas) |
| 7. Protect the arahants | Protect senior/trusted agents |

#### 21. Taṇhā 3 — Threat Model [NEW]
Three cravings as the threat-model frame.

| Craving | Meaning | Threat |
|---|---|---|
| Kāma-taṇhā | Craving for stimulus | Prompt injection, social engineering |
| Bhava-taṇhā | Craving for being | Privilege escalation, persistence |
| Vibhava-taṇhā | Craving for non-being | Destructive actions, data deletion |

#### 22. Sīla 5 — Baseline Security Rules
- No `rm -rf` of repo root (pāṇātipāta, symbolically)
- No committing secrets (adinnādāna)
- No spoofing identity (musāvāda)
- No bypassing gates (surāmeraya — losing one's senses)
- No undeclared side-effects (kāmesumicchācāra)

---

## 2. Design Principles (Derived from the 22 Frameworks)

| DP | Framework | Principle |
|---|---|---|
| DP-1 | Yoniso manasikāra | Verify before act |
| DP-2 | Mattaññutā | Right amount, not maximum |
| DP-3 | Samānattatā | Equal treatment of backends |
| DP-4 | Anattā | Non-clinging workflow |
| DP-5 | Aniccaṃ | Impermanence-aware memory |
| DP-6 | Mattaññutā + Acinteyya | Scope discipline |
| DP-7 | Attaññutā | Self-declaration of capabilities |
| DP-8 | Sīla-sāmaññatā | Communal convention |
| DP-9 | Padhāna 4 | Right effort, four directions |
| DP-10 | Ariyasacca | Decisions via Four Noble Truths |
| DP-11 | Paṭiccasamuppāda | Trace conditions backward in failures |
| DP-12 | Bhāvanā 4 | Lifecycle progression |
| DP-13 | Paññā 3 | Learn from study, thought, practice |
| DP-14 | Brahmavihāra | Equanimous error handling |
| DP-15 | Satipaṭṭhāna 4 | Four-foundation observability |
| DP-16 | Aparihāniya-dhamma | Governance for non-decline |
| DP-17 | Taṇhā 3 | Threat model via three cravings |
| DP-18 | Sīla 5 | Five baseline security rules |
| DP-19 | Kalyāṇamitta | Trust based on dhamma criteria |
| DP-20 | Kamma 3 | Audit body/speech/mind separately |

---

## 3. Application Across the Stack

```
┌──────────────────────────────────────────────────────┐
│  Aparihāniya-dhamma (Fleet Governance)               │ ← Org level
├──────────────────────────────────────────────────────┤
│  Taṇhā 3 (Threat Model) + Sīla 5 (Baseline)          │ ← Security
├──────────────────────────────────────────────────────┤
│  Bhāvanā 4 (Lifecycle) + Paññā 3 (Improvement)       │ ← Agent growth
├──────────────────────────────────────────────────────┤
│  Sāraṇīyadhamma + Kalyāṇamitta (Inter-agent)         │ ← Interconnect
├──────────────────────────────────────────────────────┤
│  Saṅgahavatthu + Brahmavihāra (UX)                   │ ← User layer
├──────────────────────────────────────────────────────┤
│  Magga 8 (Functional reqs)                           │ ← SRS
├──────────────────────────────────────────────────────┤
│  Khandha 5 (Architecture)                            │ ← Components
├──────────────────────────────────────────────────────┤
│  Satipaṭṭhāna 4 (Observability)                      │ ← Cross-cutting
├──────────────────────────────────────────────────────┤
│  Iddhipāda 4 (Engine of work)                        │ ← Runtime
├──────────────────────────────────────────────────────┤
│  Tilakkhaṇa + Kamma 3 (State & Audit)                │ ← Foundation
├──────────────────────────────────────────────────────┤
│  Paṭiccasamuppāda (Failure analysis)                 │ ← When broken
├──────────────────────────────────────────────────────┤
│  Yoniso manasikāra + Acinteyya (Method)              │ ← Thinking
└──────────────────────────────────────────────────────┘
       Ariyasacca 4 (Problem-solving cycle, end-to-end)
       Sappurisadhamma 7 (Context sensing, end-to-end)
```

---

## 4. Changes from v1.0

### 4.1 Removed forced metaphors
- v1 used *acinteyya* for "no debugging outside task scope" — strained
- v2 uses *mattaññutā* (knowing moderation) — better fit
- *acinteyya* now restricted to four cases that match its original meaning

### 4.2 Fixed cross-cutting duplication
- v1 had *yoniso manasikāra* in FR, NFR, DP — duplicated 3-4 places
- v2 lives in FR-7.7 and FR-7.17; other places reference

### 4.3 Added six new frameworks + extended existing
- Paṭiccasamuppāda → this document (no separate companion doc)
- Bhāvanā 4 → this document (no separate companion doc)
- Paññā 3 → docs/SELF-IMPROVEMENT.md
- Brahmavihāra → docs/PRD (Error UX section)
- Aparihāniya-dhamma → [`docs/en/FLEET-GOVERNANCE.en.md`](../../../../docs/en/FLEET-GOVERNANCE.en.md) (framework-root operator-facing spec drafted 2026-05-23)
- Taṇhā 3 → docs/THREAT-MODEL.md
- Kalyāṇamitta → [`interconnect/trust.md`](../../interconnect/trust.md) (spec draft v2026.5.23 — 7 declared booleans verified by `bwoc check`)
- Satipaṭṭhāna → `AGENTS.md` §10 (Observability)
- Ariya-dhana 7 → this document (maturity)
- Kamma 3 → this document (audit)
- Sīla 5 → docs/THREAT-MODEL.md (baseline)

---

## 5. Closing Note

The Buddhist frameworks here are used as engineering thinking aids, not religious interpretation. The mappings to technical concepts are useful **analogies**, not claims that "Buddhism teaches software architecture."

For depth in dhamma, study primary Buddhist sources directly.
