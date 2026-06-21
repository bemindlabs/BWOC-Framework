# 2026-06-21 — Multilingual tooling generalization (#12 / Tier 2)

The doc-naming convention already documents `<NAME>.<lang>.md` under
`docs/<lang>/` with English canonical and `<lang>` = BCP 47 / ISO 639-1
(`NAMING.en.md` rows 2–3, §`UPPERCASE.<lang>.md`). The gap was that the parity
**tooling** was hardcoded to en↔th. This removes that hardcoding so the existing
machinery works for any language, without inventing scaffolding for languages
that don't exist yet (Mattaññutā).

## What changed

- **`.claude/hooks/bilingual-reminder.sh`** — generalized from en↔th to any
  `docs/<lang>/<NAME>.<lang>.md` (and root `FILENAME.<lang>.md`). English is
  canonical: editing a translation reminds about the EN source (create if
  missing); editing the EN canonical reminds about **every translation that
  already exists**. It no longer assumes TH, and never nags to create a
  translation in an unknown language — parity is enforced only where it already
  holds. Verified across 6 cases (en/th both directions, root, template docs, and
  a no-translation file staying quiet).
- **`.claude/skills/check-bilingual/SKILL.md`** — the existence pass now iterates
  every present non-EN `docs/<lang>/` instead of hardcoding `th`. Verified clean
  against the live repo (languages present: en, th).

## Decisions

- **Generalize behaviour, keep the names.** The roadmap suggested renaming
  `bilingual-reminder`→`multilingual-reminder` and `/check-bilingual`→
  `/check-translations`, but those names are referenced by ARCHITECTURE / ROADMAP
  / FAQ in **both** languages and `/check-bilingual` is the operator's muscle
  memory. Churning ~6 cross-doc references (×2 languages) + breaking the
  invocation for a cosmetic rename isn't worth it (Mattaññutā — the smaller
  change wins). The historical name is documented as language-agnostic in each
  file's header/description.
- **Enforce parity only for existing translations.** The old hook hard-nagged
  "create the TH file"; the generic rule can't know *which* language a new EN doc
  should be translated into, so it only flags drift among translations that
  already exist (EN canonical is the one always-required direction).

## Status / deferred

- **Done:** the two tooling pieces (hook + skill) are language-agnostic; the
  convention was already documented in `NAMING.en.md`.
- **Deferred (deliberately — no consumer yet):** the manifest `languages` array
  and an `incarnate.sh --languages` flag from the Tier-2 list. Nothing reads a
  declared language set today (the tooling discovers languages from the
  filesystem), so adding the schema field + flag would be speculative scaffolding.
  Revisit when an agent actually ships a third language.

## Related

- #12 / Tier 2 (`.claude/loop-roadmap.md`); `docs/en/NAMING.en.md` (the
  already-documented convention); `.claude/hooks/bilingual-reminder.sh`,
  `.claude/skills/check-bilingual/SKILL.md`.
