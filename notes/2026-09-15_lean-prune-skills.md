# 2026-09-15 — Prune framework skills to the 5 in use

The framework shipped 23 skills under `modules/skills/`. They are prose only: no runtime loads them. The fleet uses five. The author approved removing the other 18. Every shipped manifest also had `[gates].verify = "bwoc skill verify <self>"`. That gate runs inside `bwoc skill verify --run-gates`, and the recursion guard (`BWOC_SKILL_VERIFY_INFLIGHT`) refuses the nested call, so it could never pass.

## What changed

- Removed: `ai-loop-engineer`, `counselor`, `data-engineer`, `data-scientist`, `gcloud-ops`, `illustrator`, `lawyer`, `manager`, `mathematics`, `physics`, `product-manager`, `scrum-via-jira`, `server-rag`, `software-engineer`, `soul`, `systems-engineer`, `worktree-discipline`, `writer`.
- Kept: `documenter`, `auditor`, `engineering`, `ai-dlc`, `second-brain`. Their `[gates]` table is gone.
- `modules/skill-template/manifest.toml` no longer scaffolds the self-call. A comment says the gate is optional and must not call `bwoc skill verify`.
- `audit_skill_manifest` adds a violation when `[gates].verify` invokes `bwoc skill verify` (any path to the binary, any shell separator).
- Tests: the two tests that read real skill dirs (`scrum-via-jira`, `gcloud-ops`) became `audit_skill_manifest_every_shipped_skill_passes`. The `requires_plugins` pass test is an inline fixture that covers both `jira` and `workflow`. The skill.rs dependency fixtures got neutral names. There are new tests for the recursion violation and for a non-recursive gate.
- Docs: SKILLS EN/TH use `documenter` in examples. The skill-on-plugin examples are generic ("a skill that wraps…"), with no shipped skill cited. There is a new Verification row. The skills and modules READMEs show a count of 5. The PLUGINS EN/TH sample issue summary changed. gcloud-auth SPEC EN/TH and `gcloud.sh` no longer mention the removed skill.

## Decisions

- **No gate rather than a stand-in gate.** `[gates].verify` is already optional in the spec and in `skill::run_verify`. Something like `bwoc check --all` would audit the whole fleet and say nothing about the skill. With no gate, `bwoc skill verify` reports "skipped", which is honest for prose guidance (Mattaññutā).
- **Static violation, not just the runtime guard.** The guard only fires with `--run-gates`. `bwoc check` catches the bug without executing anything. It fired on `ai-loop-engineer` before that skill was removed.
- Commit order: removal first, then validator, so each commit's test suite stays green.

## Related

- #517: SKILLS lifecycle labelled "specified, not enforced".
- [`docs/en/SKILLS.en.md`](../docs/en/SKILLS.en.md)
