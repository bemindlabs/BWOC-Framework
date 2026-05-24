# BWOC Refinement Loop — Roadmap

> **RETIRED (2026-05-25).** This refinement loop is no longer driven by any cron or `/loop`. The pivot to Phase 1 v2.0 Rust implementation superseded it (see **Discovered → "Loop superseded"**). The **live tracker is now [`docs/en/ROADMAP.en.md`](../docs/en/ROADMAP.en.md)** — trust it over the `[ ]` checkboxes below, most of which shipped under the implementation iterations recorded later in this file. This document is kept as a **historical record** of the doc-refinement and implementation iterations only; do not action its checklist. — Anattā

## Constraints (apply to every item)

- **Lean** — every line earns its place; cut bloat; deduplicate.
- **Open-source ready** — write what a public contributor needs.
- **Generic purpose** — no project-specific assumptions.
- **Any language** — design for `docs/<lang>/` extensibility, not hardcoded EN/TH.

When an item touches docs, preserve multilingual parity. When it touches a referenced-but-missing file, prefer linking the existing source of truth over duplicating.

**HELD items** (marked `🔒 HELD — needs user policy`) must NOT be auto-implemented by the loop. They commit the project to behaviors that only the user can authorize. Surface them; do not draft. See memory `feedback_policy_docs.md` for the rule.

---

## Tier 1 — Opensource hygiene (small, referenced, blocking)

- [x] **SECURITY.md** — disclosure process, scope, threat-model link, baseline rules. **Revised iter 2**: stripped invented SLAs (5/14/30/90-day) and changed `security@bemind.tech` → `info@bemind.tech` to match `CONTRIBUTING.md`. No fixed-window SLA published.
- [x] **VISION.md** — project purpose, gap, approach, success criteria, non-goals, tradeoff principles. (Added out-of-band by user request; iteration 1.)
- [x] **CODE_OF_CONDUCT.md** — BWOC-native: Sīla 5 for prohibited conduct, Brahmavihāra 4 for expected disposition. No generic OSS template imported; no invented enforcement ladder. Report contact `info@bemind.tech` matches `CONTRIBUTING.md`. Iteration 2.
- [ ] 🔒 **HELD — needs user policy** — **.github/CODEOWNERS** — referenced by `CONTRIBUTING.md`. Assigns review duty → policy. User must name owners.
- [ ] **.github/ISSUE_TEMPLATE/bug_report.md** — mechanical template; mirror `CONTRIBUTING.md` fields. Not policy.
- [ ] **.github/ISSUE_TEMPLATE/feature_request.md** — same.
- [ ] **.github/PULL_REQUEST_TEMPLATE.md** — mirror PR checklist already in `CONTRIBUTING.md`. Not policy.
- [ ] 🔒 **HELD — needs user policy** — **.github/ISSUE_TEMPLATE/config.yml** — needs Discussions URL and contact-routing decisions only the user can authorize.

## Tier 2 — Generic-purpose, any-language architecture

- [ ] **doc-naming convention** — write `modules/agent-template/conventions.md` addendum (or new `LANGUAGES.md`): file naming `<NAME>.<lang>.md`, dir layout `docs/<lang>/`, `<lang>` = BCP 47 / ISO 639-1. EN remains canonical; others are translations.
- [ ] **manifest schema for languages** — extend `modules/agent-template/config.manifest.json` with a `languages` array (default `["en"]`). Document in `conventions.md`.
- [ ] **bilingual-reminder.sh → multilingual-reminder.sh** — generalize the PostToolUse hook to scan ALL non-canonical `docs/<lang>/` directories and remind for each missing/stale translation. Rename in `.claude/settings.json`.
- [ ] **/check-bilingual → /check-translations** — rename skill, generalize to N languages, accept `<lang>` argument list.
- [ ] **incarnate.sh — `--languages` flag** — let new agents declare their language set at clone time.

## Tier 3 — Framework-level docs (root `docs/en/` and `docs/th/` currently empty)

- [ ] **docs/en/ARCHITECTURE.md** + TH — how framework, `modules/agent-template/`, and incarnated agents fit. Stack diagram lives here, not in README.
- [ ] **docs/en/ROADMAP.md** + TH — phases (currently Phase 4 per README "Status" table). Move status table out of README.
- [ ] **docs/en/GLOSSARY.md** + TH — Pali term → one-line engineering meaning. Single lookup for non-Buddhist readers.
- [ ] **docs/en/INCARNATION.md** + TH — step-by-step "how to create a new agent" (currently scattered between `incarnate.sh` comments, `README.md`, and `CLAUDE.md`).

## Tier 4 — Lean refinement (cut bloat, dedupe)

- [ ] **README.md trim** — 22-framework table duplicates `PHILOSOPHY.en.md`; stack diagram duplicates the new `ARCHITECTURE.md` (after Tier 3). Reduce README to: pitch, quickstart, links. Source of truth = `docs/`.
- [ ] **CONTRIBUTING.md sweep** — confirm every referenced file now exists (after Tier 1). Remove the `bmt-bwol-ops` hardcoded GitHub org reference — replace with a placeholder or a `SUPPORT.md` link.
- [ ] **modules/agent-template/CLAUDE.md** — verify boundary with framework-root `CLAUDE.md`; add a one-line cross-reference if needed.

## Tier 5 — Template completeness gaps

- [ ] **modules/agent-template/persona/README.md** — review; ensure it generalizes (no role/voice baked in).
- [ ] **modules/agent-template/memories/README.md** — review; ensure two-tier spec is complete and language-neutral.
- [ ] **modules/agent-template/interconnect/capabilities.md** — review; ensure capability schema is backend-neutral.
- [ ] **modules/agent-template/mindsets/** — currently empty directory; either populate a spec or write a `README.md` explaining the slot.
- [ ] **modules/agent-template/skills/** — currently empty; document the slot and conventions for agent-defined skills.
- [ ] **modules/cli/README.md** — review (one-line file at present); either flesh out or remove the module.

## Tier 6 — Optional polish (only if everything above is done)

- [ ] **docs/en/FAQ.md** + TH — extract FAQ section from README into its own file; expand.
- [ ] **examples/** — populate the empty `examples/` dir with one reference incarnation walkthrough.
- [ ] **applications/** — populate or document the empty slot.

---

## Discovered (items found during loop iterations — append, don't reorder)

- **EN/TH count mismatch in PHILOSOPHY** — `PHILOSOPHY.en.md` §1 says "22 Frameworks"; `PHILOSOPHY.th.md` §๑ says "๑๔ ประการ" (14). One is stale. Needs user direction on the correct number, then bilingual sync. Found iter 3 while adding §0.1 The Arc.
- **Loop superseded** — user pivoted from doc-first refinement to Phase 1 v2.0 implementation (Rust). Remaining roadmap items remain valid as future doc work but the cron loop is not the right vehicle for them anymore. Mechanical templates + Tier 3 framework docs can be picked up between implementation milestones.
- **`docs/README.md` and `docs/README.bad.md` are misnamed** — in `modules/agent-template/docs/`, these are not READMEs but persona EXAMPLE content (good and bad reference). Suggested rename: `examples/persona-good.md` and `examples/persona-bad.md` under a clearer `examples/` slot. Found iter 4 during README GitHub-standardization sweep.
- **`memories/README.md` and `persona/README.md` are Obsidian spec files, not landing-page READMEs** — they conform to BWOC's two-tier rule and should NOT be GitHub-standardized; doing so would violate the rule. Possibly rename to `SPEC.md` to remove the README-shape confusion. Found iter 4 during README sweep.
- **Root-level bilingual convention emerged** — `VISION.md` (canonical EN) + `VISION.th.md` (translation). Pattern for any root doc: `FILENAME.md` = EN canonical, `FILENAME.<lang>.md` = translation. This is parallel to but distinct from the `docs/<lang>/` pattern used inside the agent template. Document the convention in `conventions.md` next time it's edited. Found iter 5 (cron `/doc` loop).
- **`bilingual-reminder.sh` hook doesn't cover root-level docs** — currently scans `*/docs/en/*.en.md` only. With VISION.md / VISION.th.md pattern emerging at root, the hook should also remind for `<root>/FILENAME.md` ↔ `<root>/FILENAME.th.md`. Found iter 5.
- **`bilingual-reminder.sh` hook fires per-tool-call → false positive in parallel batches** — when EN and TH are written as parallel tool calls in the same turn, the hook may fire on the EN write before the sibling TH write completes, falsely reporting TH missing. The hook is doing its job correctly given the input timing; the fix is either (a) the hook waits briefly + re-checks, or (b) the hook is content with a warning that the operator can disregard if TH is in the same batch. Observed iter 8 (GLOSSARY EN/TH parallel write). Low priority — informational only.
- **README's stack diagram is CONCEPTUAL, not implementation** — earlier iter-7 roadmap note said "move stack diagram from README to ARCHITECTURE." Wrong. The README diagram shows the 22 Buddhist-framework groupings (concept). ARCHITECTURE got its own implementation-layer diagram. Both legitimately coexist. Tier 4 "README trim" remains a separate item (the README still has bloat to address with user direction). Corrected iter 9.
- **VERSION.md added** — mirrors `Cargo.toml` workspace version (`0.1.0`); explains SemVer policy + phase-vs-version distinction. Update in the same commit whenever workspace version changes. Iter 5.
- **CHANGELOG.md added** — Keep a Changelog 1.1.0 format; `[Unreleased]` section captures all Phase 1 v2.0 work to date (open-source hygiene, spec additions, Rust scaffold, tooling, conventions, known issues). Iter 6.
- **Three crate READMEs added** (`crates/bwoc-core/README.md`, `crates/bwoc-cli/README.md`, `crates/bwoc-agent/README.md`) — Rust workspace convention; each cross-references the arc (§0.1 PHILOSOPHY). Batched in one iteration since they are coherent and near-identical in shape (Mattaññutā — right amount of work per fire). Iter 7.
- **`docs/en/GLOSSARY.en.md` + `docs/th/GLOSSARY.th.md` added** — single alphabetized table, ~60 entries spanning all 22 frameworks plus the arc triad, the 4 abidings, the 8 Magga limbs, the 3 marks, the 3 cravings, the 5 precepts, and standalone principles. Bilingual EN/TH parity in same turn. Also populates the previously-empty framework-root `docs/en/` and `docs/th/`. **CHANGELOG.md updated** to record crate READMEs + GLOSSARY together. Iter 8.
- **`docs/en/ARCHITECTURE.en.md` + `docs/th/ARCHITECTURE.th.md` added** — implementation stack with own diagram (distinct from the conceptual stack in README/PHILOSOPHY), `bwoc spawn` information flow, backend-neutrality, multilingual structure across 3 surfaces (docs / root metadata / CLI locales), trust boundary table. Did NOT move the README stack diagram — my prior roadmap note was wrong: README has the *conceptual* stack (22 frameworks), ARCHITECTURE has the *implementation* stack. Both legitimately exist. CHANGELOG updated. Iter 9.
- **`docs/en/INCARNATION.en.md` + `docs/th/INCARNATION.th.md` added + scattered duplicates trimmed** — canonical step-by-step with prerequisites, 6-step walkthrough, add-a-backend, multilingual setup, verification checklist, and post-incarnation reading path. **Also trimmed**: root README "Getting Started" (replaced stale `cp -r` recipe with `incarnate.sh` + link), agent-template README "Incarnating a New Agent" (kept quickstart, removed 4-line duplicate, added link). Lean — every duplicate of the walkthrough now points to the single source. CHANGELOG updated under both Added and Changed. Iter 10.
- **Auto-version hook installed** (out-of-band, direct user directive) — `.claude/hooks/auto-version.sh` PostToolUse Write|Edit. Restructured `VERSION.md` to expose `Software-Version` + `Document-Version` + `Last-Updated` (UTC ISO 8601). Independent versioning (per user direction). Software domain auto-bumps `Cargo.toml` workspace patch; document domain auto-bumps `VERSION.md` Document-Version patch; both stamp Last-Updated. Self-managed files guarded against self-trigger (Cargo.toml, Cargo.lock, VERSION.md, .claude/*, target/*). Registered alongside bilingual-reminder. Pipe-test passed AND live-proof: document → 1.0.2 on CHANGELOG.md edit (system reminder confirmed). CHANGELOG updated.
- **Workspace + central memory spec added** (out-of-band, direct user directive) — `docs/en/WORKSPACE.en.md` + `docs/th/WORKSPACE.th.md`. Defines `.bwoc/` marker, `workspace.toml` + `agents.toml` schemas, validation rules ("complete before work" with exit code `2`), CLI surface (`bwoc init`, `bwoc workspace info/validate`, `--workspace` flag with 5-level precedence), `~/.bwoc/` central per-user memory (config + memory + workspaces registry + logs), and memory scope hierarchy (per-agent → per-workspace → per-user → Tier 2). bwoc-cli/README.md updated to declare the workspace commands and `--workspace` flag. **Spec-only this turn**; implementation in Rust is a separate task awaiting user go-ahead. CHANGELOG updated.
- **auto-version.sh bug fixed** — initial implementation used GNU-only `0,/regex/s||...|` sed syntax which silently failed on macOS BSD sed. Discovered when Cargo.toml stayed at `0.1.0` after a `crates/*` edit that the hook claimed bumped it to `0.1.1`. Replaced with portable `s|^version = "X.Y.Z"$|version = "X.Y.Z"|` (anchored start+end; only the workspace.package line matches since dependency entries have leading whitespace). Pipe-test verified bumps both Cargo.toml and VERSION.md. Manually reconciled Cargo.toml = 0.1.1 to match VERSION.md before fix.
- **NAMING standard added** (out-of-band, direct user directive) — `docs/en/NAMING.en.md` + `docs/th/NAMING.th.md`. 12 categories, rule definitions, quick decision tree, audit grep snippets. Establishes the note pattern `YYYY-MM-DD_<title>.md` (ISO 8601 + underscore + kebab-case) with three valid locations (repo / workspace / per-user). `/check-naming` skill flagged as a future implementation. CHANGELOG updated.
- **`docs/en/ROADMAP.en.md` + `docs/th/ROADMAP.th.md` added** — phase-by-phase plan. Phase 1 v2.0 uppāda has Completed / In Progress / Remaining-for-ship subsections with spec-doc cross-references. Phases 2/3/4 each have DoD. Cross-cutting concerns enumerated. README Status table trimmed to a high-level summary that links to ROADMAP. CHANGELOG updated. Iter 12.
- **`docs/en/FAQ.en.md` + `docs/th/FAQ.th.md` added** — 7 categories (Conceptual, Project Mechanics, Setup, Multi-Language and Multi-Backend, Conventions, Operations, Contributing), ~20 Qs. Extracts README's 3 Qs and expands with newcomer Qs surfaced by every spec doc written this session. README FAQ trimmed to top-3 summary + link to full. CHANGELOG updated. Iter 13.
- **Small cleanup sweep — both remaining queued items closed.** Iter 14:
  - `modules/agent-template/conventions.md` — added pointer to `NAMING.en.md` as comprehensive `*.md` standard; softened validation checklist "File names are kebab-case.md" → "Markdown file names follow NAMING.en.md (12 categories)"; renamed "Files & Directories" subsection to "Directories" since file naming now lives in NAMING. Added NAMING to See Also.
  - `modules/agent-template/docs/th/PHILOSOPHY.th.md` — fixed `## ๑. หลักธรรมหลัก ๑๔ ประการ` → `## ๑. หลักธรรมหลัก ๒๒ ประการ` (was stale; 22 verified canonical).
  - CHANGELOG updated under Changed section with both items.

### Implementation loop `419aa20f` (every 3 min, prompt: `do to complete`)

- **Iter 1** — `modules/*` audit per user `ตรวจสอบ modules/* ด้วย` directive. Filled 3 empty top-level READMEs (`modules/`, `modules/plugins/`, `modules/skills/`) and added 2 Obsidian SPEC.md to empty agent-template slots (`mindsets/`, `skills/`). Stopped per "address modules/* findings + STOP" choice.
- **Iter 2** — `bwoc check` implemented. Full parity port of `check-agent-neutrality.sh` into `crates/bwoc-cli/src/check.rs`. Subcommand wired, tests pass, fmt+clippy clean. Live run against `modules/agent-template` returns 15 PASS / 0 violations.
- **Iter 2 addendum (out-of-band user directive)** — Manifest input behavior spec'd. User: "Manifest จะต้องรับ inputs จากผู้ใช้ เพื่อตั้งค่า agent profile เริ่มต้น และแก้ไขได้ภายหลัง". Updated `INCARNATION.en.md` + `INCARNATION.th.md` with two new sections: (a) "Setting the Manifest" specifies `bwoc new` accepts fields via flags AND via interactive TTY prompts (non-TTY = fail-fast); reads field schema from `config.manifest.json` `requiredConfig` so adding a new field is a schema change, not a CLI change. (b) "Editing the Manifest After Incarnation" specifies direct file edit as canonical; `bwoc manifest set/get` deferred to Phase 2 conditional on real friction (Mattaññutā).
- **Iter 3** — `bwoc new` implemented (flags-only) + `bwoc-core::manifest::Manifest` type + `scripts/install.sh` one-command installer. Live end-to-end verified: `bwoc new test-agent-foo --target /tmp/test-agent-foo --role ... --primary-model ... --*-cmd ...` → 4 symlinks + resolved manifest + `bwoc check` returns 15 PASS / 0 violations. Software 0.1.7 → 0.1.14. Tests: 4 unit tests in bwoc-core + bwoc-cli. Install directive ("add one command install this software to user's computer") addressed by `scripts/install.sh` + README + crate README updates.
- **Iter 4** — interactive TTY prompts for `bwoc new` complete. Uses `std::io::IsTerminal` (no new dep). On TTY: prompts missing required fields one by one with `{key} ({description}): ` format (description from template's `requiredConfig`). On non-TTY: collects ALL missing fields in one pass, fails fast with exit 2 and comma-separated list (no partial blocking on stdin). Two new unit tests for fail-fast + template-description loading. Live-verified both paths via `< /dev/null` piping. Software 0.1.14 → 0.1.17.
- **Iter 5** — `bwoc spawn` implemented. Minimal exec: validates path has `AGENTS.md`, `cd`'s into agent dir, exec's backend CLI via `Command::status()` (cross-platform). `--backend` ValueEnum (claude/gemini/codex/kimi; default claude). Extra args after `--` passthrough. Backend-not-found returns actionable error. 4 new unit tests; live verification with real kimi CLI successfully launching in agent template. **Phase 1 v2.0 uppāda DoD = REACHED** — `bwoc new` → `bwoc check` → `bwoc spawn` chain works without any shell scripts. Software 0.1.17 → 0.1.21.

### Phase 1 v2.0 uppāda — DoD checklist

- ✓ `bwoc new` (flags + interactive + non-TTY fail-fast)
- ✓ `bwoc check` (full parity with shell script)
- ✓ `bwoc spawn` (minimal exec, propagates exit code)
- ✓ `bwoc init` (creates workspace + agents.toml + agents/; idempotent w/ `--force`)
- ✓ `bwoc workspace info` (dumps resolved workspace + agent count)
- ✓ `bwoc workspace validate` (5 spec rules; exit 0/2)
- ✓ `bwoc-agent` real runtime (manifest-driven liveness)
- ✓ `bwoc new` auto-registers in enclosing workspace's `agents.toml` (`--backend` flag added; ancestor walk; duplicate refusal; best-effort)
- ✓ `~/.bwoc/` directory creation (auto on first CLI invocation; directory + empty `config.toml`; memory/workspaces/logs deferred)
- ✓ `/check-naming` in CI (`.github/workflows/docs.yml`: root-level + `docs/<lang>/` + notes/ gates with `::error::` annotations)
- ✓ bilingual-reminder hook coverage (root-level pairs + reverse direction for docs/<lang>/)
- ✓ Non-policy issue + PR templates (`.github/ISSUE_TEMPLATE/{bug_report,feature_request}.md` + `.github/PULL_REQUEST_TEMPLATE.md`; BWOC-flavored fields; mirror CONTRIBUTING checklist)
- ✓ `bwoc list` (reads workspace `agents.toml`; full WORKSPACE.en.md resolution chain with ancestor walk; ROADMAP "Remaining for ship" item closed)
- ✓ `--lang` → Project Fluent wiring (infrastructure + 1 message as proof; full string conversion deferred — touches many literals)
- ✓ Ancestor-walk resolution promoted to `workspace info` / `validate` (backward compatible; same chain as `bwoc list` and `bwoc new`'s auto-registration)
- ✓ Fluent conversion: `bwoc init` (7 message keys EN+TH; t_with helper; lang plumbed via InitArgs; caught the Fluent dot-in-id syntax gotcha mid-iter)
- ✓ Fluent conversion: `bwoc list` (5 message keys EN+TH; empty + 4 col labels; lang plumbed via ListArgs::into_runtime; known TH-column-width cosmetic)
- ✓ Fluent conversion: `bwoc spawn` (1 message key with $backend + $path; lang plumbed via SpawnArgs::into_runtime; live-verified with real codex CLI)
- ✓ Fluent conversion: `bwoc workspace info` (9 message keys — header + 7 field labels + agent-row; InfoArgs carries lang; same TH alignment cosmetic as list)
- ✓ Fluent conversion: `bwoc workspace validate` (5 message keys — header + PASS/FAIL labels + 2 summaries; ValidateArgs carries lang; finding descriptions stay English to avoid .ftl balloon)
- ✓ Fluent conversion: `bwoc check` (9 message keys — header + target + 3 labels + 2 summary + 2 tail; check::run signature changed to (&Path, &str); findings stay English)
- ✓ Fluent conversion: `bwoc new` (10 message keys — incarnated + target + reg status + next-steps + 4 numbered + prompt format; all major fns thread &FluentBundle; symlink lines stay literal)
- ✓ Fluent conversion: `bwoc-agent` (duplicated i18n module + own locales/; 7 keys — 6 liveness + 1 error; BWOC_LANG/LANG/en chain). **All 8 Fluent surfaces complete.**
- ✓ Runtime works from any directory (embedded template via `include_dir!` + 5-level resolution chain; default_target updated for non-framework cwd)
- ✓ `scripts/bump-version.sh` for manual major/minor/patch bumps (patch still auto via hook)
- ✓ `scripts/install.sh` upgrade-in-place (`--force` + existing-install detection)
- ✓ Phase 1 v2.0 ROADMAP cleanup EN+TH (table renamed to "Shipped in Phase 1 v2.0" with ✓ marks)

**Phase 1 v2.0 — DoD reached.** Only HELD policy items + user's release-tag decision remain.

- **Iter 6** — `bwoc init` + `bwoc-core::workspace` types + `toml = "0.9"` workspace dep. Workspace types are TOML-serde with load/save; init creates the canonical `.bwoc/workspace.toml` + `.bwoc/agents.toml` + `agents/` layout per `WORKSPACE.en.md` spec. UTC ISO 8601 stamp implemented via stdlib + small Gregorian conversion to avoid chrono/time. 7 new unit tests (3 in bwoc-core::workspace, 4 in bwoc-cli::init). Live-verified: init creates files, refuses without `--force`, accepts with `--force`. Software 0.1.21 → 0.1.29. Mid-iter bug: my first `iso8601_format_is_stable` test expected the wrong timestamp (used 1779004800 thinking it was 2026-05-22T06:00:00Z; actual is ~T08:00:00Z). Fixed by switching to epoch-anchor fixtures (0, 86399, 86400, 2024 leap day).
- **Iter 7** — `bwoc workspace info` + `bwoc workspace validate` implemented. `info` dumps resolved workspace + config + agent count + per-agent rows from `agents.toml`. `validate` runs the 5 rules from `WORKSPACE.en.md` (`.bwoc/` exists, `workspace.toml` parses + required fields, version is strict SemVer X.Y.Z, `agents.toml` parses, `agents_dir` exists); exits 0/2. Short-circuits early on structural failures. 4 new unit tests (SemVer validation, missing `.bwoc/`, clean workspace, bad SemVer). Live-verified 3 scenarios: happy (7 PASS), degraded (6 PASS / 1 FAIL on missing agents/), corrupt TOML (1 PASS / 1 FAIL on parse error with short-circuit). Software 0.1.29 → 0.1.33.
- **Iter 8** — `bwoc-agent` real runtime. Replaces the "I am alive" stub with manifest-driven output: reads `config.manifest.json` from cwd via `bwoc-core::manifest::Manifest::load_from_path`, prints structured liveness (`I am alive: <agentId>` + role + model + fallback + memory + version). Exit 2 if cwd has no manifest (actionable message); exit 1 on parse failure; exit 0 on success. Pure-data `liveness_banner` separated from `main` for testability. 2 new unit tests (required fields present, optional fallback omitted when None). Live-verified inside `/tmp/agent-live` (created via `bwoc new`): all six lines correct; non-agent dir gives the right error + exit 2. Software 0.1.33 → 0.1.34. Single bump because main.rs is the only software-domain file changed.
- **Iter 9** — `bwoc new` auto-registers in workspace `agents.toml`. Extracted shared `utc_now_iso8601` helpers from `init.rs` to new `crates/bwoc-cli/src/util.rs` (one helper, two callers now). Added `--backend` ValueEnum flag to `bwoc new` (default claude). After successful manifest save, ancestor-walks from `target.parent()` for `.bwoc/workspace.toml`; if found, loads `AgentsRegistry`, refuses duplicate agent_id (`NewError::DuplicateRegistration`), appends `AgentEntry { id, path (relative to workspace root), backend, incarnated, status }`, saves back. Best-effort: registration failures log warning but don't fail the incarnation. 1 new ancestor-walk unit test. Live-verified both inside (`/tmp/wks/agents/agent-alpha` → registered; agents.toml has entry; `bwoc workspace info` shows 1 agent) and outside (`/tmp/agent-orphan` → "No workspace found in ancestors"). Software 0.1.34 → 0.1.51 (large jump because util extraction + new.rs registration + main.rs flag passthrough touched many software-domain files; one Edit failed mid-stream due to fmt drift and was recovered with a re-read).
- **Iter 10** — `~/.bwoc/` bootstrap. New module `crates/bwoc-cli/src/user_home.rs` with `ensure_initialized()` (creates directory + empty `config.toml` with header) and `bwoc_home()` (resolution only). Cross-platform `$HOME` (Unix) / `%USERPROFILE%` (Windows) lookup; no new dep. Called from `main` at startup as best-effort — failure logs warning but does not block commands. Memory/, workspaces.toml, logs/ deferred to commands that actually need them (Mattaññutā). 2 unit tests (creation + idempotency-without-overwrite) using `unsafe` `set_var("HOME", ...)` under a Mutex since Rust 2024 marks env-mutation unsafe. Live-verified: fake-home creates files; unset-HOME prints warning and continues. Software 0.1.51 → 0.1.54.
- **Iter 11** — `/check-naming` wired into CI. New `.github/workflows/docs.yml` runs 3 gates on every PR/push touching markdown: root-level allowlist, `docs/<lang>/` UPPERCASE check (mindepth 2 so it skips slot-level examples like `docs/project-example.md`), and notes/ YYYY-MM-DD_<title> check. Each emits `::error::` GitHub annotations on violation and exits non-zero. **Pre-flight against the live repo found 2 false positives** in my prior grep patterns: `CLAUDE.local.md` (Claude Code convention, needs explicit `.local` suffix exemption) and the `docs/project-example.md`/`reference-example.md` files at slot level (needed mindepth 2 to skip the docs/ root). Refined the greps; NAMING.en.md + NAMING.th.md + SKILL.md all updated to keep documented greps identical to what CI runs (Sīlasāmaññatā — single source of truth). Live-verified all 3 gates return empty against current state. No software bumps this iter (only `.yml` + `.md` edits, no Rust code touched).
- **Iter 12** — `bilingual-reminder.sh` extended. Adds (a) reverse direction for `docs/<lang>/` (editing TH reminds about EN canonical) and (b) root-level `FILENAME.md` ↔ `FILENAME.th.md` (e.g., `VISION.md` ↔ `VISION.th.md`). Root-level canonical→translation only fires if the translation already exists, so unpaired files like `CHANGELOG.md` don't generate noise. Out-of-repo paths exit silently (matches the auto-version.sh fix from iter 10). Pipe-tested 8 scenarios — all four positive paths emit correct JSON, all four negative paths exit silently. No software bumps (only `.claude/*` edits + doc updates).
- **Iter 13** — Three non-policy issue/PR templates filled in: `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/feature_request.md`, `.github/PULL_REQUEST_TEMPLATE.md`. BWOC-flavored bug-report fields (backend choice, arc phase, surface); Ariyasacca-shaped feature-request (Problem/Solution/Alternatives) with optional principle alignment; PR template checklist mirrors CONTRIBUTING.md PR Checklist verbatim PLUS adds the CI gates (fmt/clippy/test/naming-audit) and the bilingual-parity / manifest-schema invariants. Correctly classified as NON-policy: they're mechanical forms over existing CONTRIBUTING content, not new commitments. Three policy-bearing items remain HELD: `CODEOWNERS` (assigns review duty) and `ISSUE_TEMPLATE/config.yml` (contact routing).
- **Iter 14** — `bwoc list` implemented. Caught two items in ROADMAP "Remaining for ship" I'd missed (focused too long on the notes table): `bwoc list` and Fluent wiring. Picked list since it closes a real workspace-surface gap. Reads `.bwoc/agents.toml` via `AgentsRegistry::load`; prints id/status/backend/path table or `(no agents in workspace ...)` if empty. Full WORKSPACE.en.md resolution chain (explicit `--workspace` → `BWOC_WORKSPACE` env → ancestor walk → cwd → exit 2). 1 new unit test (ancestor walk + explicit-path precedence). Live-verified 4 scenarios (empty, populated via flag, ancestor walk from subdir, non-workspace dir). Mid-iter clippy bump: `print_literal` lint on the table-header `"PATH"` literal — inlined per clippy suggestion. Software 0.1.54 → 0.1.61.
- **Iter 15** — `--lang` → Project Fluent wiring (infrastructure + proof). New `crates/bwoc-cli/src/i18n.rs` module with `bundle_for(lang)` and `t(bundle, key)`. Locale files (`locales/<lang>/cli.ftl`) embedded via `include_str!` (no runtime disk lookup needed — important for distributed binaries). Default-Fluent Unicode isolation marks disabled for plain terminal output. Unknown languages fall back to EN. Missing keys return `«missing key: <key>»` rather than panicking. Added `default-help-hint` message to both `en/cli.ftl` and `th/cli.ftl`; converted the default-no-subcommand `println!` literal to use it. **Scope deliberately limited to infrastructure + one message** (Mattaññutā) — full conversion of the 100+ literals across `check`/`new`/`spawn`/`init`/`workspace`/`list`/`bwoc-agent` is a follow-up. 4 new unit tests (EN/TH content, unknown-lang fallback, missing-key marker). Live-verified 4 paths: default EN, `--lang th`, `BWOC_LANG=th`, `--lang ja` (fallback). Software 0.1.61 → 0.1.68.
- **Iter 16** — Ancestor-walk resolution promoted to `workspace info` / `workspace validate`. Replaced the cwd-only `resolve_root` calls with `find_workspace_root` (from iter 14). Dropped the now-unused `resolve_root` helper. Backward compatible: explicit positional path still works. New behavior: from any workspace subdir, `bwoc workspace info` and `bwoc workspace validate` find the enclosing workspace via the same chain `bwoc list` uses (explicit → `BWOC_WORKSPACE` env → ancestor walk → cwd → exit 2 with actionable message). Live-verified 4 scenarios. No new tests (the function `find_workspace_root` already has its iter-14 unit test). Software 0.1.68 → 0.1.70.
- **Iter 17** — Fluent conversion: `bwoc init` (first subcommand). Added `t_with(bundle, key, &[(name, value)])` for named-arg interpolation. 7 new `init-*` message keys in en/cli.ftl + th/cli.ftl (success title, three created-file lines, next-steps header, validate/new suggestions). Plumbed `lang` through `InitArgs` so `init::run()` builds its own bundle. **Mid-iter gotcha caught**: Fluent identifiers may NOT contain `.` — my first attempt with `init.success-title` panicked at FluentResource parsing ("ExpectedToken('=')"); fixed by renaming to `init-success-title` (hyphens are legal). Cosmetic regression noted: Fluent strips leading whitespace from single-line values, so the `"  + "` indents in the EN output are flattened. Restorable with the `{""}` placeable trick if we revisit. Live-verified both EN ("Initialized BWOC workspace at: ...") and TH ("สร้าง BWOC workspace ที่: ..."). 34 tests pass. Software 0.1.70 → 0.1.91.
- **Iter 18** — Fluent conversion: `bwoc list`. 5 new `list-*` keys (empty msg + 4 col labels). Empty message uses `$path` interpolation via `t_with`. TH translates `STATUS` → `สถานะ`; the other column labels stay as English ASCII (`ID`/`Backend`/`Path`) since they're programmer-standard terms. `lang` plumbed via `ListArgs::into_runtime`. Known cosmetic: Rust's `{:<10}` pads by byte count not visual width, so the Thai column header alignment is slightly off (fix would need the `unicode-width` crate; deferred). Live-verified 4 scenarios (EN/TH × empty/populated). Software 0.1.91 → 0.1.98.
- **Iter 19** — Fluent conversion: `bwoc spawn`. 1 new `spawn-exec-status` message key with `$backend` + `$path` args. TH uses preposition `ใน` ("in"). `lang` plumbed via `SpawnArgs::into_runtime`. Error path (BackendNotFound, PathMissing, NotAnAgent, Io) stays English. Live-verified by spawning the real codex CLI in `modules/agent-template` from both EN and TH locales — status line correctly interpolates the backend name + path. Software 0.1.98 → 0.1.105. Spawn is the smallest subcommand so far (1 user-visible success line).
- **Iter 20** — Fluent conversion: `bwoc workspace info`. 9 new keys (`info-header` with `$path`; 7 `info-label-*` field labels; `info-agent-row` with `$id`+`$status`+`$path`). `info()` now takes the bundle as a parameter; `run_info()` builds it from `args.lang`; `InfoArgs.lang` plumbed via main.rs. Deferred bwoc-agent this iter (separate crate, needs its own i18n setup — postponing the architecture decision to extract i18n into bwoc-core vs duplicate). Known cosmetic carried over: dynamic labels of varying width break the fixed-position colon alignment in the output (was hidden when labels were hardcoded). Live-verified EN + TH. Software 0.1.105 → 0.1.111.
- **Iter 21** — Fluent conversion: `bwoc workspace validate`. 5 new keys (`validate-header` with `$path`; `validate-label-pass`, `validate-label-fail`; `validate-summary-success` with `$passes`; `validate-summary-failure` with `$passes`+`$violations`). TH: `PASS`→`ผ่าน`, `FAIL`→`ไม่ผ่าน`, summaries translated. `print_validation_report()` takes the bundle as a parameter. Finding descriptions stay English (rule-specific; ~10 strings would balloon the .ftl — defer unless requested). `ValidateArgs.lang` plumbed via main.rs. Live-verified 3 scenarios (EN happy, TH happy, TH degraded with deleted agents/ → "6 ผ่าน, 1 ละเมิด — แก้ก่อนใช้งาน workspace นี้", exit 2). Software 0.1.111 → 0.1.117.
- **Iter 22** — Fluent conversion: `bwoc check`. 9 new `check-*` keys (header; target with `$target`; 3 status labels PASS/WARN/FAIL; success summary with `$warnings` + tail line; failure summary with `$violations`+`$warnings` + tail line). TH labels: `ผ่าน`/`เตือน`/`ไม่ผ่าน`; "Neutrality check passed." → "การตรวจสอบ neutrality ผ่าน". `print_report()` takes the bundle as a parameter. `check::run()` signature changed from `(&Path)` to `(&Path, &str)` to thread the lang. Finding descriptions (~15 rule-specific lines) stay English — translating those would balloon the .ftl with marginal benefit (most are technical strings like "AGENTS.md contains {{agentId}}"). Live-verified EN + TH against `modules/agent-template` — all 15 PASS lines render with localized PASS prefix; summary lines fully localized. Software 0.1.117 → 0.1.122.
- **Iter 23** — Fluent conversion: `bwoc new`. 10 new `new-*` keys (incarnated + target headers; registered / not-registered workspace status; next-steps header + 4 numbered steps; prompt-format for interactive). All major functions in new.rs now thread `&FluentBundle<FluentResource>` (run, incarnate, resolve, resolve_one, print_report). Symlink lines stay literal (data, not labels). Mid-iter bugs caught: missing `use crate::i18n;` import (cascaded into 11 compile errors) — fixed; two unit tests needed `lang: "en"` in `args_with_role_only()` fixture and `&bundle` into the `resolve()` call. Live-verified EN ("Incarnated agent: agent-alphaen / Target: ...") and TH ("สร้าง agent: agent-alphath / เป้าหมาย: ..."). Software 0.1.122 → 0.1.135.
- **Iter 24** — Fluent conversion: `bwoc-agent` (last remaining surface). Picked **Option A: duplicate i18n module + own locales/** (lean — avoids bwoc-core refactor; defers DRY concern to when modules actually drift). New `crates/bwoc-agent/src/i18n.rs` mirrors bwoc-cli's i18n.rs with bwoc-agent's own FTL constants. Added `resolve_lang()` (BWOC_LANG → LANG → en; matches bwoc-cli's chain minus the --lang flag since bwoc-agent doesn't take CLI args). 2 new FTL files (`locales/en/agent.ftl` + `locales/th/agent.ftl`) with 7 keys: 6 liveness lines (alive, role, model, fallback, memory, version) + 1 missing-manifest error. bwoc-agent/Cargo.toml inherits fluent-bundle + unic-langid from workspace deps. main.rs's `liveness_banner` now takes `&FluentBundle`; tests updated (was 2, now 7). Mid-iter warning fixed: `t()` is dead in bwoc-agent (only `t_with` used in main) — added `#[allow(dead_code)]` since it's pub API kept for future no-arg messages. Live-verified inside `/tmp/agentlive`: EN ("I am alive: agent-agentlive ...") and TH ("ฉันยังมีชีวิตอยู่: agent-agentlive ..."). Software 0.1.135 → 0.1.141.

**Phase 1 v2.0 Fluent conversion — COMPLETE** across all 8 CLI + agent surfaces.

---

## Phase 2 + 3 implementation (post-DoD, post-iter-24)

A second long arc of cron-driven iters (`c045cc3d` → `d1937f6a`) on user directives "improve cli to easy to use" and "implement all roadmap phases, focus easy to use". Summary grouped by theme; CHANGELOG.md + `git log` are the per-commit ledger.

### Lifecycle verbs (Phase 3 vaya + new state machine)

- **`bwoc retire`** — remove agent from registry, optional `--keep-files`. TTY confirm + `--yes`.
- **`bwoc stop`** — set status=stopped; later wired to send STOP via socket when daemon is alive.
- **`bwoc start`** — set status=active AND spawn `bwoc-agent --serve`. Idempotent across all 4 (status × daemon) state combinations. `--no-daemon` opt-out.
- **`bwoc workspace prune`** — reconcile phantom registry entries vs orphan agent dirs. `--apply` removes safe drift.

### Messaging stack (Phase 3 sammā-vācā Phase 0)

- **`bwoc send <agent> <msg>`** — append JSONL envelope to `<agent>/.bwoc/inbox.jsonl`.
- **`bwoc inbox <agent>`** — read companion. `--limit N` · `--json` · `--watch` (live tail) · `--clear` (truncate with confirm).
- **`bwoc list` INBOX column** — count of pending envelopes per agent (— for 0).
- **Inbox cursor persistence** — `<agent>/.bwoc/inbox.cursor` so `bwoc-agent --serve` restart doesn't skip messages received while offline.
- **Daemon-side inbox announce** — `--serve` polls inbox every ~100ms, prints `bwoc-agent: inbox ← user: <msg>` to stderr as envelopes arrive.

### Process supervision (Phase 2 ṭhiti)

- **`bwoc-agent --serve`** daemon mode — writes PID + Unix socket at `.bwoc/agent.{pid,sock}`. ctrlc signal handler; graceful cleanup. `inbox.cursor` for restart-safe message replay.
- **Line-text IPC protocol over Unix socket** — debuggable with `nc -U`:
  - `PING\n` → `PONG\n` (used by `bwoc ping <agent>`)
  - `STATUS\n` → `OK uptime_secs=N pid=N\n` (used by `bwoc status` for uptime display)
  - `STOP\n` → `OK shutting down\n` + daemon exits (used by `bwoc stop`)
- **`bwoc ping <agent>`** — CLI client for PING.
- **`bwoc status` runtime indicator** — `● running (pid N, uptime 5m12s)` / `○ not running` via PID file + signal-0 + socket query.
- **`bwoc list` runtime column** — `●/○` prefix per row; `--running` filter to narrow to live daemons.
- **`bwoc chat <agent>`** — auto-resolves backend + path from registry, exec's `bwoc spawn`. `--tmux` opens in new tmux window.

### TUI dashboard (Phase 2 → 4 progressive)

- **Phase 0** — ratatui shell, draws title + footer, q/Esc/Ctrl-C quit, alt-screen restore.
- **Phase 1** — agents pane populated from `agents.toml`; ↑↓/jk nav; `r` refresh.
- **Phase 2** — detail pane reusing doctor health probes.
- **Phase 3** — Fluent i18n (22 `dash-*` keys per locale).
- **Phase 4+** — auto-refresh every 2s; runtime/uptime/inbox count in detail; persona scope + mindsets/skills/memories counts; workspace projects + notes counts in banner; `t` hotkey opens tmux new-window running `bwoc spawn`; transient `last_action` feedback in footer.

### Doctor extensions

Beyond the original env + workspace checks:
- **Stale-PID sweep** (`agent.pid` exists, process gone) — `--auto` removes.
- **Stale-socket sweep** (`agent.sock` exists, no live owner) — `--auto` removes.
- **Stale-cursor sweep** (`inbox.cursor` malformed / out-of-bounds / orphan) — `--auto` removes.

### Read-only command surface (--json across)

All these emit human tables by default, structured JSON with `--json`:
`bwoc list` · `bwoc status [name]` · `bwoc workspace info` · `bwoc workspace validate` · `bwoc check [path]`.
Stable shapes; consumer-safe across locale.

### Refactor: shared `crate::livecheck`

After 5 callers accumulated near-identical copies of `signal_zero_alive` + `running_pid` + `query_uptime` + `format_uptime` + `inbox_count`, consolidated into one module. Pure code health, -31 lines net, 6 new unit tests.

### UX polish

- **`install.sh`** — installs BOTH `bwoc` + `bwoc-agent`; `--check`/`--uninstall`/`--help` modes; pre-flight PATH warning; usable quickstart in tail output.
- **`bwoc help` topics** — 8 total: `getting-started` · `backends` · `workspace` · `manifest` · `arc` · `lifecycle` · `daemon` · `messaging`. Each cross-links the others.
- **`bwoc completion <shell>`** — bash/zsh/fish/powershell/elvish via `clap_complete`.
- **`bwoc init` writes `.gitignore`** — excludes daemon ephemerals (`agent.pid`/`sock`/`inbox.cursor`); keeps `inbox.jsonl` tracked by default (with opt-out comment).
- **`bwoc new` interactive pickers** — `--role` catalog · per-backend model catalog · stack-detected lint/format/test/build defaults · NEW: `--scope` / `--out-of-scope` for persona substitution + `--mindsets` / `--skills` for stub seeding.
- **`bwoc list` filters** — `--status` · `--backend` (ValueEnum) · `--running`.
- **README Getting Started** refreshed to match the real install + lifecycle flow.

### Bugs surfaced + fixed

- `default_target` in `bwoc new` placed agents at framework root when template auto-resolved from `modules/agent-template/` AND a workspace existed at root — workspace-aware branch reordered to win.
- Framework repo's `.gitignore` had drifted (`/.bwoc/`, `.bwoc/cache/`, `.bwoc/local/` redundancy; typo `BEOC`; invalid `---` pattern). Consolidated into one block with policy comment contrasting framework-repo vs `bwoc init` user-workspace defaults.
- Multiple mid-iter clippy slips caught + fixed in follow-ups: `useless_format` · `doc_lazy_continuation` · `doc_overindented_list_items` · `ptr_arg` · `trim_split_whitespace` · `print_literal` · `dead_code`. `cargo install` doesn't gate on clippy; future workflow could add a `cargo clippy --all-targets -- -D warnings` step pre-commit.

### Adjacent bumps + house-keeping

- `crates/bwoc-cli/src/livecheck.rs` introduced (consolidation); 6 unit tests.
- Manifest schema gained `scope_description` + `out_of_scope` (optional).
- `IncarnationReport` exposes `persona_filled` + `mindset_stubs` + `skill_stubs`; `print_report` surfaces them inline.
- Auto-version hook still firing across `.rs`/`.toml`/`.md` writes; software version reached ~0.1.387 by end of post-DoD arc.

### Currently HELD (unchanged from Phase 1)

- 🔒 `.github/CODEOWNERS` — review assignments.
- 🔒 `.github/ISSUE_TEMPLATE/config.yml` — Discussions URL + contact routing.

### Next planned (in priority order, post-tmux-iter)

1. TH localization of lifecycle messages (`stop`/`start`/`retire`/`ping`/`send`/`inbox`/`chat`).
2. `unicode-width` padding fix for Thai column alignment across `list`/`status`/`dashboard`.
3. Agent → agent SEND (needs Kalyāṇamitta 7 trust model first; spec doc).
4. Multi-OS release matrix + signed binaries (CI work).
5. `bwoc help` topic localization (TH).

Cron `d1937f6a` still firing every 5 min on the same dual directive.

---

## Completed log

- **SECURITY.md** — disclosure process, scope, baseline rules. Links the existing `THREAT-MODEL.en.md` instead of duplicating it (Mattaññutā — right amount). Iteration 1. **Revised iter 2** to strip invented SLAs and unify on `info@bemind.tech`.
- **VISION.md** — purpose, gap addressed, approach, 1-year / 3-year success, non-goals, tradeoff principles. Out-of-band addition; not project-specific bloat because the framework root has identity (BWOC) while the template stays generic. Iteration 1.
- **CODE_OF_CONDUCT.md** — Sīla 5 (prohibited conduct) + Brahmavihāra 4 (expected disposition). No identity-political language; no enforcement-ladder invention. Aligned with VISION.md non-goals. **Sīla** named in the doc itself (the framework is the framework). Iteration 2.
- **Loop posture change** — cron `7bb13808` cancelled. Loop is **paused** until user directs policy on remaining policy-bearing items. Mechanical items (issue/PR templates) can resume on demand without the loop.
