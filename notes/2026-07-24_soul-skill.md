# 2026-07-24 — `soul` framework skill

Adds a framework skill (`modules/skills/soul/`, `domain/identity`) for an agent holding its enduring core — values, voice, principles, boundaries — consistently across every task, so it is recognizably *itself*. The deliberate BWOC paradox: a consistent stream of character (Sīla) + resolve (Adhiṭṭhāna) held *without* clinging to a fixed ego (Anattā).

## What changed

- New `modules/skills/soul/` (`manifest.toml` + `SPEC.md`), L1.
  - `embody(context)` — act from the core: values + voice + principles, expressed in the doing, not announced.
  - `reflect(action)` — catch drift + realign to the core, correcting ego-free (no self to defend, per Anattā).
- The one skill whose teardown is deliberately *not* a release — the core persists across tasks (unlike worktrees/context, let go per Anattā).

## Decisions

- **Threads dilution vs ego-clinging.** Sīla/Adhiṭṭhāna keep character *consistent*; Anattā keeps it *unclinging* — like a river, recognizably itself though every drop changes. The skill names both failure modes explicitly.
- **Model-agnostic identity** — the same soul rides whichever backend (Samānattatā, literally). No hardcoded vendor.
- **Complements, not duplicates:** `counselor` = outward warmth, `soul` = inward consistency; `writer` = voice-as-craft, `soul` = voice-as-identity.

## Related (links)

- `modules/skills/soul/`; siblings `counselor`, `writer`; `modules/agent-template/docs/en/PHILOSOPHY.en.md` (+ `.th`) — Anattā / Sīla / Adhiṭṭhāna.
