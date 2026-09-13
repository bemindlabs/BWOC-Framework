# 2026-09-13 — `bwoc init` keeps an existing agent registry

Surfaced during the 3.0 fleet migration. srv1544731 had `.bwoc/agents.toml` (agent-caretaker, served by `bwocd`) but no `.bwoc/workspace.toml`, so `bwoc migrate` found no workspace. Running `bwoc init` in place would have replaced the registry with an empty one and unregistered the agent `bwocd` serves. The migration went ahead with a manual backup and restore of `agents.toml`.

## What changed
- `init` writes the default `agents.toml` only when none exists.
- Test `init_keeps_an_existing_agents_registry` covers both cases: a pre-existing registry with no workspace.toml, and `--force` over an existing workspace.

## Decisions
- Applies with and without `--force`. The `AlreadyExists` error text says `--force` overwrites *workspace.toml*, so wiping the registry was never part of that contract.
- The "created agents.toml" line in the success report is left as is. Changing it would need a new Fluent key in both EN and TH for a cosmetic difference.
