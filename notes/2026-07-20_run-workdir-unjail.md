# 2026-07-20 — `bwoc run --workdir`: opt-in un-jail for cross-project tasks

`bwoc run` spawned the agent with `cwd = agent_dir` (its own directory). For an ambient backend (`claude -p`) that cwd is the effective project root and headless mode won't touch files outside it (no interactive approval); for harness backends the same path was passed as `--workdir`, the FS-jail root. Net effect: a headless agent could not edit shared workspace files (`projects/`, `wiki/`). Added an opt-in `--workdir` so a task that legitimately needs cross-project scope can widen the run cwd — bounded to the workspace.

## What changed

- `run::RunArgs` gains `workdir: Option<PathBuf>`; CLI flag `--workdir` on `bwoc run`.
- `execute()` resolves the run cwd via new `resolve_workdir(workspace, workdir, agent_dir)`:
  - `None` → `agent_dir` (unchanged jailed default).
  - `Some(p)` → absolute as-is, else `workspace.join(p)`; canonicalized, must be an existing **directory inside the workspace root** (both sides canonicalized so `..` can't escape). Else `RunError::BadWorkdir`.
  - `--workdir .` therefore means "run at the workspace root".
- `build_command` signature split: `config_dir` (manifest/identity — always `agent_dir`) vs `work_dir` (process cwd + harness `--workdir` arg). So the fix reaches **every** backend (Samānattatā), not just Claude — harness backends now jail at the widened dir too.
- 5 new unit tests: default jails to agent_dir; `--workdir .` widens to root; harness `--workdir` arg follows the run cwd; escaping (`..`) refused; nonexistent refused. MockCommandRunner now captures cwd.

## Decisions

- **Opt-in, default unchanged.** The jailed default is the safe posture (Sīla — minimal blast radius); widening is an explicit per-invocation choice, not a config default.
- **Bounded to the workspace.** A resolved cwd outside the workspace root is refused — this is "run anywhere in *this* workspace", not "run anywhere on disk". Keeps the un-jail from becoming an arbitrary-FS escape.
- **Single option, not two.** Rejected a separate `--scope agent|workspace` enum on top of `--workdir` — `--workdir .` already expresses "workspace root" and `--workdir <path>` covers project dirs. Mattaññutā: one flag, full coverage.
- **Manifest stays at `agent_dir`.** The agent's identity/config never moves with the run cwd.

## Alternatives considered

- Local-inbox auto-drain in the `--serve` daemon (auto-start a headless run on a trusted local inbox message) — the *other* half of the reported problem. Deliberately **not** taken this round (owner chose the `--workdir` fix only); it carries a real security tradeoff (autonomous headless turns on an ambient backend) and belongs in its own gated, opt-in change.
- Changing the default cwd to the workspace root — rejected; widens blast radius for every existing run.

## Status / deferred

- Shipped behind `--workdir`; no behavior change without the flag.
- Deferred: daemon local-inbox auto-drain (auto-start). Separate proposal.

## Related (links)

- `crates/bwoc-cli/src/run.rs` (`resolve_workdir`, `build_command`), `crates/bwoc-cli/src/main.rs` (`RunCliArgs`).
- Reported via the bwoc-vscode-extension / control-center fleet UX (agent `ratu` inbox stuck + fs-jail).
