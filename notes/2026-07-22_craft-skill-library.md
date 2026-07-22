# 2026-07-22 — craft skill library (10 framework skills)

Adds ten `type/skill` framework skills under `modules/skills/` — a library of reusable *craft/persona capabilities* an agent can enable per role. Each maps to Buddhist principles the framework already uses, exposes two operations, is environment-neutral (no hardcoded backend/model/vendor), declares `maturity = L1`, and cross-links its siblings. Requested by the owner as a batch.

## The ten skills

| Skill | domain/ | Operations | Principle |
|---|---|---|---|
| **writer** | content | draft · revise | Sammā-vācā (right speech), Mattaññutā |
| **illustrator** | media | compose_prompt · generate | Sammā-diṭṭhi, Mattaññutā (backend-neutral `<imageBackend>`) |
| **manager** | coordination | plan · delegate | Saṅgha, Mattaññutā, Sīlasāmaññatā |
| **counselor** | wellbeing | listen · advise | the four Brahmavihāra (Mettā/Karuṇā/Muditā/Upekkhā); warmth ≠ sycophancy |
| **auditor** | assurance | audit · verify_finding | Yoniso Manasikāra, Sacca (adversarial verify, no false positives) |
| **engineering** | engineering | implement · harden | Sīla (gates), Appamāda, Anattā (worktree) |
| **mathematics** | reasoning | derive · check_result | Yoniso Manasikāra, Sacca (units/limits/second method) |
| **physics** | science | model · estimate | Sammā-diṭṭhi, Yoniso Manasikāra (dims + order-of-magnitude) |
| **documenter** | documentation | document · sync | Sīlasāmaññatā, Sacca (accurate + kept in sync; sibling of writer) |
| **lawyer** | governance | review · flag_risk | Sīla, Sacca + hard humility clause (route legal decisions to a human) |

## Decisions

- **Framework skills, not per-agent slot skills.** These are reusable across agents and installs → `modules/skills/` (enable per agent with `bwoc skill enable`), matching `worktree-discipline` / `second-brain` / `server-rag`.
- **Environment-neutral by construction.** The only external surfaces (`illustrator`'s image backend) are operator-configured placeholders, no hardcoded vendor — so `bwoc check` / Samānattatā stay satisfied. Framework-skill SPECs are English (repo convention; TH parity applies to `docs/`, not module SPECs).
- **Every skill is grounded in a principle the framework already maps**, not invented ethics — so the library reads as one coherent voice.
- **Humility clauses where competence has a boundary**: `counselor` and `lawyer` both explicitly route weighty decisions (medical/mental-health, binding legal) to a qualified human rather than improvising authority.
- **Cross-linked siblings** (writer↔illustrator↔documenter, auditor↔engineering↔manager, mathematics↔physics, counselor↔lawyer) so an agent picks the right one and knows the neighbours.

## Status / deferred

- All ten at **L1**; not enabled on any agent yet (`bwoc skill enable <name>` per role is an operator step). `bwoc skill list` + `bwoc skill verify` pass for each (static).
- `accounting-api` (write to an external system of record) remains out of scope as a *skill* — it belongs as a `plugin` kind (like `gws`/`jira`).

## Related (links)

- `modules/skills/{writer,illustrator,manager,counselor,auditor,engineering,mathematics,physics,documenter,lawyer}/`
- Kin: `modules/skills/{second-brain,server-rag,worktree-discipline}/`; `docs/en/SKILLS.en.md`.
