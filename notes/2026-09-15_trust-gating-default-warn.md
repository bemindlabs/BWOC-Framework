# 2026-09-15 — Trust gating on by default (warn-only)

An audit found that `BWOC_TRUST_GATING` was set nowhere: no fleet host, CI job, script or systemd unit. The daemon's Kalyāṇamitta quality gate therefore never ran, and `bwoc check` was the only place `requiredTrust` had any effect. The author approved turning the gate on by default in warn mode while keeping refusal opt-in.

## What changed
- `crates/bwoc-agent/src/trust.rs`: new `GatingEnv::parse` (`0`/`off`/`false` → Disabled, `1` → Enforce, anything else including unset → DefaultWarn). `TrustContext::build` caps the manifest's effective mode at `Warn` under DefaultWarn. Manifest `off` still passes through.
- `TrustContext` gains `gating_default`, which drives the startup notice, and `warned`, a bounded `(sender, missing)` seen-set read through `first_warning`. `main.rs` logs `trust_warn` only on the first sighting of a pair. Warnings are never written to `inbox.refusals.jsonl`.
- `evaluate` is **not modified**. The default only changes what step 2 (the quality gate) returns, from `Pass` to `Warn`.
- Docs: `trust.md`/`.th` gain a §Daemon gating precedence table and a v2.1 revision entry. `routing.md`/`.th`, `README.md` and the `bwoc-agent` README are reworded. `COMPATIBILITY.en/th` gain the rule "a default may tighten only as far as a warning".

## Decisions
- **Manifest `refuse` is capped under the default.** The requested precedence was env > manifest > default. Honouring a manifest `refuse` without the env var would refuse envelopes that 3.1 delivers, which breaks the 3.x compatibility contract. So a manifest can relax the default (`off`) but only `=1` escalates it. Scaffolded agents carry `requiredTrust` with no `mode`, which v1 reads as refuse, so this covers most of the fleet.
- **`=1` keeps its exact meaning**: the manifest's effective mode, refuse included. No `refuse` env value was added because none existed before.
- **`BWOC_SIGNING_MODE=off` suppresses the default.** With signing off and the gate inert, `evaluate` returns on its fast path before resolving the sender. Turning the gate on there would run resolution, and its can't-verify arms (`unknown_sender`, `no_workspace`, and so on) would start refusing. When signing is on, which is the ratified default, resolution, signature verification and replay checks already run whatever the gating setting, so the default adds no new refusal path.
- Dedup lasts for the daemon run and is capped at 4096 keys, cleared when full. There is no time-based rate limit because senders form a small, operator-managed set.

## Alternatives considered
- Downgrading can't-verify refusals to warnings under signing-off plus the default. Rejected: `Warn` carries no reason field, and this would silently change a security path.
- Writing warnings to the refusals sidecar with `outcome: "warn"`. Rejected: `bwoc inbox` and `fleet health` treat every row as a refusal.

## Related
- `crates/bwoc-agent/src/trust.rs` (`GatingEnv`, `TrustContext::build`, `first_warning`)
- `modules/agent-template/interconnect/trust.md` §Daemon gating
- `docs/en/COMPATIBILITY.en.md` §Deprecation
