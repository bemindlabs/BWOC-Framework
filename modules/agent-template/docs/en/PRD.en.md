# PRD — Product Requirements Document

## Agent Base Profile (Structured by Ariyasacca 4 — The Four Noble Truths)

| | |
|---|---|
| **Document** | PRD.en.md |
| **Version** | 2.0 |
| **Date** | 2026-05-22 |
| **Bilingual Pair** | PRD.th.md |
| **Philosophy Reference** | PHILOSOPHY.en.md |
| **Reference Implementation** | `kla-ondemand/atlas-agent-oracle-template` |

> **Document spine:** the Four Noble Truths — Dukkha, Samudaya, Nirodha, Magga
> Supplemented by: Sappurisadhamma 7 (context), Saṅgahavatthu 4 (UX), Iddhipāda 4 (metrics), Tilakkhaṇa (constraints).

---

## Part 1 — Dukkha (The Problem)

### 1.1 Executive Summary

Agent Base Profile is a **backend-neutral template for creating AI coding agents** whose primary role is to "remember and connect." One repo, one agent — each agent is cloned from this template and given its own persona, skills, and mindset.

The entire system is designed against Buddhist principles — see PHILOSOPHY.en.md for the full framework.

### 1.2 Four Dukkha of Today's AI Coding Agents

#### Dukkha 1 — Amnesia
Every session starts from zero. Lessons, decisions, and accumulated context evaporate when the chat closes. Users repeat themselves.

#### Dukkha 2 — Vendor Lock-in
Agents are coupled to a single LLM provider's tooling. Switching providers means rewriting from scratch.

#### Dukkha 3 — Multi-Agent Collision
Multiple agents in the same repo collide on branches, share state through `git stash`, and contaminate each other's work.

#### Dukkha 4 — Structural Chaos
Each agent invents its own layout. No standard means no inter-agent coordination.

---

## Part 2 — Samudaya (Root Causes)

### 2.1 Root-Cause Analysis

| Dukkha | Surface Cause | Deep Cause |
|---|---|---|
| Amnesia | No persistent storage | Lack of *sammā-sati* — no cross-session memory mechanism |
| Vendor lock-in | Backend-specific tooling | Lack of *samānattatā* — backends not treated equally |
| Collision | Shared working directory | Lack of *anattā* — clinging to one's branch |
| Chaos | No central convention | Lack of *sīla-sāmaññatā* — no shared rules |

### 2.2 Craving (Taṇhā) That Generates the Dukkha
- Craving for short-term convenience → using `git stash` instead of a worktree
- Craving for ownership → holding branches, no cleanup
- Craving for the old → trusting memory without verification
- Craving for wide reach → working outside scope (*mattaññutā*)

---

## Part 3 — Nirodha (Vision of Success)

### 3.1 Vision Statement
Every AI coding agent in an organization shares the same skeleton, so knowledge compounds and agents collaborate as a fleet rather than as isolated tools.

### 3.2 Measurable End-State

| Outcome | Measurement |
|---|---|
| Agent recalls prior decisions across sessions | ≥ 95% on prior-decision tests |
| Backend switch with zero file edits | Same task → equivalent output on 5 backends |
| Three concurrent agents, zero collisions | CI smoke test over 100 task-pairs |
| New agent ready to commit within 30 minutes | Onboarding stopwatch |
| Agents reach consensus via protocol | Two agents complete a consensus exchange |

### 3.3 System Aspirations (Chanda)
- **Aspiration to remember** — keep only what matters; quality over volume
- **Aspiration to connect** — find relations across contexts
- **Aspiration to release** — finish without clinging

---

## Part 4 — Magga (The Path)

### 4.1 The Eightfold Path (full detail in SRS)

| Pillar | In the System |
|---|---|
| Sammā-diṭṭhi | Clear persona and identity |
| Sammā-saṅkappa | Task planning via the Four Noble Truths |
| Sammā-vācā | Inter-agent communication protocol |
| Sammā-kammanta | Worktree isolation + scoped commits |
| Sammā-ājīva | Trust model + neutrality |
| Sammā-vāyāma | Verification gates (the Four Padhāna) |
| Sammā-sati | Memory system, Tier 1 + Tier 2 |
| Sammā-samādhi | Stable session lifecycle |

### 4.2 Phased Roadmap by the Four Padhāna

| Phase | Padhāna | Work |
|---|---|---|
| Phase 1 | Saṃvara (guard against new ill) | MVP: AGENTS.md + symlinks + Tier 1 memory + worktrees + scripts |
| Phase 2 | Pahāna (abandon existing ill) | Remove legacy patterns: no stash, no shared dirs, enforce conventions |
| Phase 3 | Bhāvanā (cultivate new good) | Tier 2 memory, interconnect, self-improvement loop |
| Phase 4 | Anurakkhanā (sustain existing good) | Reference agent gallery, fleet dashboards, signed templates |

---

## Part 5 — Sappurisadhamma 7 (Knowing Seven Dimensions of Context)

### 5.1 Stakeholder & Context Analysis

#### Dhammaññutā — Knowing the Cause
The underlying principle is "remember and connect," not "do tasks faster." The agent's role is to accumulate and link knowledge.

#### Atthaññutā — Knowing the Result
End goals: reduce onboarding cost of new agents, reduce context-rebuild cost for users, enable multi-agent collaboration.

#### Attaññutā — Knowing Oneself (Personas)

| Persona | Role | Primary Need |
|---|---|---|
| **Agent Author** | Builds new agents | Fast clone-and-customize |
| **Agent Operator** | Runs agents day-to-day | Predictable behavior, no collision, persistent memory |
| **Platform Maintainer** | Maintains the template | Validation tools, trust model |
| **LLM CLI** | Consumes the template at runtime | Backend-neutral instructions |

#### Mattaññutā — Knowing Moderation (Non-Goals)
- **Not** a runtime or orchestrator
- **Not** a memory database (Tier 2 is pluggable)
- **Not** an agent marketplace
- **Not** language-specific
- **Not** a chat UI

#### Kālaññutā — Knowing Time
- Use when building a new agent that must remember across sessions
- Not for one-shot scripts or ad-hoc queries
- Not for agents without a backing repo

#### Parisaññutā — Knowing the Community
The target community: teams running multiple AI coding assistants in shared repos, within a single organization, with disciplined git workflows and a need for governance.

#### Puggalaññutā — Knowing the Individual
The user is a developer or AI engineer familiar with git, markdown, shell, and JSON — not an end-consumer.

---

## Part 6 — Saṅgahavatthu 4 (UX Principles)

### 6.1 Dāna — Generosity
- Default configuration is complete and usable
- Skills, mindsets, persona scaffolds are provided
- Examples are concrete

### 6.2 Piyavācā — Pleasant Communication
- Error messages name the offending placeholder
- README has crisp steps
- Documentation is bilingual

### 6.3 Atthacariyā — Beneficial Action
- Scripts not only act, they validate
- Validation tells you what is wrong and how to fix it

### 6.4 Samānattatā — Equal Treatment
- All backends are equal — symlinks all point to the same `AGENTS.md`
- No "primary backend"

---

## Part 7 — Iddhipāda 4 (Success Metrics)

| Path | KPI |
|---|---|
| **Chanda** | Agent author satisfaction ≥ 4/5 |
| **Viriya** | Task completion rate ≥ 90%; retry-on-fail enabled |
| **Citta** | 100% verification-gate compliance on merged PRs |
| **Vīmaṃsā** | Self-improvement metrics tracked; monthly skill progression |

### Supporting KPIs

| Metric | Target |
|---|---|
| Time-to-first-commit (new agent) | ≤ 30 min |
| Branch collision rate | 0 over 100 task-pairs |
| Backend portability | 5 backends, identical behavior |
| Memory recall accuracy | ≥ 95% |
| Neutrality check pass rate | 100% on official templates |
| Task-log completeness | 100% |

---

## Part 8 — Tilakkhaṇa (Constraints and Letting Go)

### 8.1 Aniccaṃ — Everything Changes
- Memory carries timestamps and a pruning policy
- Conventions can change via template versioning
- The list of supported backends will change — design for additions and removals

### 8.2 Dukkhaṃ — The Old State Cannot Endure
- Stale branches = dukkha → clean up on cadence
- Stale memory = dukkha → mine into Tier 2 or delete
- Stale conventions = dukkha → review on a schedule

### 8.3 Anattā — Not a Self
- The agent does not own its branches or worktrees
- The template is "no one's" — every fork is equal
- Memory is not "truth" — current code is truth

---

## Part 9 — Acinteyya 4 (What This Document Does Not Address)

| Unthinkable | Out of Scope |
|---|---|
| Buddha-visaya | Which LLM provider is "better" |
| Jhāna-visaya | LLM internals |
| Kamma-vipāka | Long-term business outcomes |
| Loka-cintā | Global AI ethics and governance |

---

## Part 10 — Risks and Mitigations

| Risk | Buddhist Lens | Mitigation |
|---|---|---|
| Symlinks break on Windows | Aniccaṃ — environments change | Document WSL workaround |
| Agents bypass worktree isolation | Craving for speed | Hooks in `.claude/settings.json` block it |
| Memory grows unbounded | Craving to accumulate | 200-line cap (*mattaññutā*) + Tier 2 |
| Forked templates drift from neutrality | Saṅkhāra — formations shift | `bwoc check` in CI |
| Placeholders left unsubstituted | Carelessness | Manifest-driven validation |

---

## Part 11 — Open Questions

1. Should agents be able to fork themselves mid-task?
2. What is the canonical schema for inter-agent messages?
3. Should the template adopt semver?
4. Should signed templates (sigstore-style) be supported?

---

## Appendix A — Mapping to PHILOSOPHY

| Section | Dhamma Framework |
|---|---|
| Part 1 — Dukkha | Ariyasacca 1 |
| Part 2 — Samudaya | Ariyasacca 2 |
| Part 3 — Nirodha | Ariyasacca 3 |
| Part 4 — Magga | Ariyasacca 4 + Magga 8 |
| Part 5 — Context | Sappurisadhamma 7 |
| Part 6 — UX | Saṅgahavatthu 4 |
| Part 7 — KPIs | Iddhipāda 4 |
| Part 8 — Constraints | Tilakkhaṇa |
| Part 9 — Out of scope | Acinteyya 4 |
| Part 10 — Risks | Taṇhā + remedies |
| Part 11 — Open Q | (open, no single framework) |

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
