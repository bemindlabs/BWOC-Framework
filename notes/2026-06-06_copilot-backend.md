# 2026-06-06 — GitHub Copilot CLI as the 6th backend

`copilot` joins claude/agy/codex/kimi/ollama (Samānattatā — all backends equal).
Requested by the architect; CLI behaviour verified against GitHub's docs before
wiring (binary `copilot`; programmatic mode `copilot -p "<prompt>"`;
`--no-ask-user` required non-interactively; `--allow-all-tools` scoped by
GitHub's guidance to sandboxed/container environments; **reads `AGENTS.md`
natively** for custom instructions).

## What changed

- **`spawn::Backend::Copilot`** — `cli_name "copilot"`, `display_name`,
  multi-model `models()` menu (Claude + GPT slugs, best-effort — vendor menu
  evolves, free text accepted), no `reasoningEffort` flag (Copilot exposes
  none). Interactive `bwoc spawn`/`bwoc chat` exec it in the agent dir; since
  Copilot reads `AGENTS.md` natively, the neutral source of truth works as-is.
- **`bwoc run` headless support** — `copilot -p "<task>" --no-ask-user`. The
  permissive `--allow-all-tools` is deliberately NOT passed (fail-safe: tool
  calls that would prompt are refused rather than auto-approved or hanging).
  Copilot is now the **second** vendor backend with headless support (after
  Claude) — partial HV3-6 progress.
- **`parse_backend`** (chat.rs + run.rs) accepts `"copilot"`; clap `ValueEnum`
  derives `--backend copilot` everywhere automatically.
- **Surfaces updated**: doctor's vendor-CLI PATH probe + warning, `bwoc help
  backends` table (+ Copilot row), handbook spawn/agents sections (EN+TH) +
  symlink list, `WORKSPACE.en/th.md` backend comment.
- **Template**: `COPILOT.md → AGENTS.md` symlink added — following the
  OLLAMA.md/OPENAI.md precedent (in the template, *not* added to `bwoc check`'s
  required-symlink list, so existing incarnated agents keep passing checks).

## Decisions

- **Verify-then-wire**: invocation flags and the AGENTS.md-native behaviour were
  confirmed from GitHub's published docs (Yoniso Manasikāra), not assumed.
- **No `--allow-all-tools` in headless runs**: GitHub scopes it to containers;
  bwoc's posture is fail-safe (mirrors the harness's `ask`→deny in non-TTY). An
  operator who wants it can layer it themselves when sandboxed.
- **Model list is advisory** (the picker/catalog is "convenience, not a
  whitelist" per its doc comment) — Copilot's slugs follow its `/model` picker
  and will drift; free text is accepted.

## Status / deferred

- Done pending CI. Deferred: a `--model` passthrough for Copilot runs (flag not
  verified), and full HV3-6 parity for codex/agy/kimi headless.

## Related

- `crates/bwoc-cli/src/{spawn,run,chat,doctor,help,handbook}.rs`,
  `docs/{en,th}/WORKSPACE.*.md`, `modules/agent-template/COPILOT.md`
- Sources: GitHub Copilot CLI docs/blog (verified 2026-06-06)
