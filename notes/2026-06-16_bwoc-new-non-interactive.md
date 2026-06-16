# 2026-06-16 — `bwoc new --yes` non-interactive scaffolding (issue #298)

`bwoc new` already worked non-interactively *if* you passed every required field
(role, primaryModel, and all four gate commands). The friction the issue hit:
on a non-TTY stdin a missing gate command (`lintCmd`/`formatCmd`/`testCmd`/
`buildCmd`) is a hard `MissingFields` error — so scripted team creation had to
spell out four cmd flags per agent (or use `--project` to seed them), and people
fell back to hand-writing `config.manifest.json` + symlinks + `agents.toml`.

## What changed

- **`bwoc new --yes`** (alias `--non-interactive`). With it, only `--role` and
  `--primary-model` stay required (they have no sensible default); the four gate
  commands fall back to their **stack-detected default** (`suggested_cmd`, e.g.
  `cargo clippy …` for a Rust project) or **`true`** when the stack is unknown.
- `resolve_one`'s non-TTY branch now *accepts a supplied suggestion* instead of
  always erroring — so the `--yes` defaults flow through the same path the TTY
  prompts use.
- Without `--yes`, non-TTY behaviour is unchanged (fail-fast listing every
  missing field) — the stricter default is preserved.

`bwoc new` already stamps `incarnated` + `status` and writes the symlink set, so
once it runs the agent is registry-recognized; the issue's "mark incarnated by
hand" pain was a symptom of *not* being able to run it non-interactively.

## Decisions

- **role + primaryModel are never auto-defaulted.** Inventing them would scaffold
  a misconfigured agent silently. The gate commands, by contrast, have a safe
  no-op (`true`) and a stack default, so defaulting them is harmless.
- **Opt-in flag, not implicit.** A bare non-TTY run still fails fast — `--yes`
  is the explicit "I accept the defaults" signal (matches the issue's ask for
  `--non-interactive`/`--yes`).
- A spec-file form (`--from spec.toml`) was considered and deferred: flags +
  `--json` already cover scripted provisioning; a spec file is additive if a
  real need appears (Mattaññutā).

## Related

- issue #298; `crates/bwoc-cli/src/new.rs`, `crates/bwoc-cli/src/main.rs`
