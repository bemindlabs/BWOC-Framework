# 2026-07-22 — role skill set (5 framework skills)

Adds five profession/role framework skills under `modules/skills/`, extending the craft library toward named engineering + product roles. Same conventions as the craft set: two operations each, environment-neutral, `maturity = L1`, Buddhist-principle grounding, cross-linked siblings, **correct** repo-root wikilink depth (`../../../docs/en/`).

## The five skills

| Skill | domain/ | Operations | Principle | Distinct from |
|---|---|---|---|---|
| **product-manager** | product | discover · define | Yoniso Manasikāra, Mattaññutā | `manager` (what-to-build vs how-to-coordinate) |
| **systems-engineer** | systems | architect_system · assure_reliability | Paṭicca-samuppāda, Sammā-diṭṭhi | designs the *composition*, not a component |
| **software-engineer** | software | design_component · review_code | Sammā-diṭṭhi, Sīla, Yoniso Manasikāra | design+review bracket around `engineering`'s implement+harden |
| **data-engineer** | data | build_pipeline · ensure_data_quality | Sacca, Sīla | data-plane sibling of `systems-engineer` |
| **data-scientist** | data-science | analyze · build_model | Yoniso Manasikāra, Sacca | leans on `mathematics` + `data-engineer` |

## Decisions

- **Roles distinct from crafts.** `software-engineer` = design + review (the professional bracket); `engineering` = implement + harden (the build craft). `product-manager` = *what/why*; `manager` = *how* (team/task coordination). Each role names its distinction in its abstract to avoid overlap confusion.
- **Honesty is the through-line.** `data-scientist` (no leakage, report uncertainty), `data-engineer` (assert quality, fail loud), `software-engineer` (review the actual diff), `systems-engineer` (design for failure) all encode Sacca / Yoniso Manasikāra — the fleet's verify-before-trust rule, specialised.
- **Correct wikilink depth this time.** `../../../docs/en/` (repo root is 3 up from `modules/skills/<name>/`) after Copilot flagged the off-by-one (`../../docs/`) the earlier skill batches inherited from `worktree-discipline`. A follow-up could align the older skills' cosmetic wikilinks; they resolve by note name in Obsidian regardless.

## Status / deferred

- All five at L1; not enabled on any agent. Together with the craft set + `ai-dlc`, they let an agent be assigned a coherent role (e.g. a "software-engineer" agent enables software-engineer + engineering + auditor + documenter).

## Related (links)

- `modules/skills/{product-manager,systems-engineer,software-engineer,data-engineer,data-scientist}/`
- Kin: the craft set (`writer`…`lawyer`) + `ai-dlc`.
