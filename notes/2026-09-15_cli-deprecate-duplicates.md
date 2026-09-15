# 2026-09-15 — Deprecate duplicate CLI entry points

Group D of the lean pass (author-approved: "alias + deprecation warning in 3.2, remove in 4.0"). Five deprecated forms now rewrite onto a canonical command and print one stderr line. Their stdout, `--json` and exit codes are unchanged. Six candidate clusters from the audit were re-verified against the handlers; three were true duplicates, three were not.

## What changed

- `util::deprecated(old, new)` is the single warning path: one line to **stderr**, silenced by `BWOC_NO_DEPRECATION_WARNINGS=1`.
- `canonicalize(Commands) -> Canonical` in `main.rs` maps a deprecated command onto the canonical `Commands` value **before** dispatch, so both forms go through one match arm. The old arms are now `unreachable!`.
- Deprecated → canonical:
  - `notes | retro | research <verb>` → `doc <verb> notes | retrospectives | research`
  - `tasks [--agent] [--state] [--json]` → `task list --all …`. `task list` gained `--all`, `--agent` and `--state`, and `<TEAM>` is optional with `--all`.
  - `memory t2-search <q> <agent>` → `memory search <q> <agent> --tier 2`. `memory search` gained `--tier 1|2` (default 1) and an optional `<AGENT>`.
- `--help` about text marks each deprecated command `(deprecated → …)`. `spawn` / `chat` / `run` / `agent` got about text that tells them apart.
- Docs: COMPATIBILITY EN/TH gained the CLI deprecation rule and a "Deprecated in 3.2" table. NAMING EN/TH and LOOP-ENGINEERING EN/TH, both READMEs, the deep-memory README and the tier-2-noop SPEC now use the canonical forms.

## Decisions

- **Rewrite, not delegate.** Converting the parsed command, instead of calling the handler from a second arm, makes "same handler" a structural fact. It can also be tested at parse level: the deprecated argv, once canonicalized, must `Debug`-equal the canonical argv.
- **`Canonical { command, deprecated: Option<_> }` rather than `Result<Rewritten, Commands>`**: clippy's `result_large_err` rejects a `Commands`-sized `Err`.
- **`--agent` / `--state` use `conflicts_with = "team"`, not `requires = "all"`.** clap did not reject `task list t --agent x` with `requires` on a bool flag, so the filter would have been silently ignored.
- **Tier 2 + `--json`, or an agent with tier 1, is exit 2** rather than being ignored. Neither combination existed before, so no caller relies on it.

## Not deprecated (audited, kept)

- **`status` / `fleet` / `info` / `sessions` / `list`**: different data, not aliases. Against one workspace, `list` shows registry state (STATUS, UPTIME, INBOX); `status` shows health and model; `fleet status` shows online, pending and last-message; `info` shows the version/release card; `sessions` shows a process scan. Bare `fleet` = `fleet status` is one command's default, not a second entry point.
- **`peer feedback` vs `send`**: `send` has no `--kind`, forced peer routing, or require-signature flags. Exposing those would widen `send`'s surface, not reduce it.
- **`spawn` / `chat` / `run` / `agent run`**: four code paths. `chat` calls `spawn::run` but cannot pass `-- <extra>` args or an explicit `--path`/`--backend`. `run` is headless capture. `agent run` is a root privilege drop. Only the help text changed.

## Verification

A baseline binary built from `origin/main` was compared with this branch in a real fixture (one agent, a team with tasks, a seeded note). There were 15 deprecated-vs-canonical cases and 5 unchanged canonical forms. stdout and exit codes were identical in every case (baseline old = new old = new canonical); stderr differs only by the warning line. The env var suppresses it.
