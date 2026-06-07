# 2026-06-07 — Phase 5 charter: saṃvara (trust-boundary & sandbox hardening)

The tianting council (recurring `/loop`, chair agent-yudi) chartered a new roadmap phase. Phase 5 — *saṃvara* — hardens the untrusted-ingress surface that Phase 3's chat-connectors opened into `bwoc-harness`. ROADMAP (EN + TH) gained a `## Phase 5` section; the 8 DoD gates were filed as `tianting` tasks (`t1`–`t8`), each `--requires-plan` (Pavāraṇā).

## What changed

- `docs/en/ROADMAP.en.md` + `docs/th/ROADMAP.th.md` — new Phase 5 section (DoD, ratified contract, 8-gate checklist, deferred-and-fenced list) + Current-Status line updated.
- `.bwoc/teams/tianting` task list — 8 tasks from the DoD gates, dependency-wired, plan-gated.

## Decisions

- **Phase 5, not Phase 4.** Phase 4 (Reference Agents + Fleet) already exists and is adoption-driven; the council initially mis-scoped this work as "Phase 4" off a flawed grep. Corrected against the live ROADMAP (Yoniso manasikāra). Security hardening is its own phase.
- **Dhamma tag = saṃvara** (indriya-saṃvara, guarding the sense-doors) — restraint at the boundary where untrusted input could become effect. Continues the uppāda → ṭhiti → vaya lineage.
- **Boundary at tool-effect, not ingestion.** Untrusted text → LLM is a policy/injection problem; only effectful tools need containment (luban, council pass #2).
- **Isolation unit = OS process + layered capability gate.** Not container (breaks self-hosted-on-any-Unix), not pure gate (harness intentionally spawns processes). Multi-tenant harness, single-tenant ephemeral sandbox per `(connector, conversation)` turn.
- **Trust tags taint-propagate** — confused-deputy defense; tianting amendment to luban's "read-only is safe" (egress-as-read + laundering holes).
- **DoD gates are `--requires-plan`** — security-critical, so each gate's claimant submits a plan for yudi (lead) approval before completion.

## Alternatives considered

- Fold into a cross-cutting security epic (THREAT-MODEL + tasks) without a phase — rejected; the work is large and milestone-shaped enough to be a phase.
- Container/microVM isolation as the v1 unit — rejected for v1 (breaks self-hosted Unix portability); kept as a deferred pluggable backend.

## Status / deferred

- Charter + DoD landed; **no gate implemented yet** — `t1`–`t8` open in the tianting list.
- Fenced out of the DoD: seccomp-bpf, Landlock FS-jail, container/microVM backend, macOS Seatbelt parity (macOS v1 = rlimits + privilege-drop only, gap documented).

## Related

- `docs/en/ROADMAP.en.md` §Phase 5 (+ `.th.md`)
- `modules/agent-template/docs/en/THREAT-MODEL.en.md` — Kāma-taṇhā / Vibhava-taṇhā vectors
