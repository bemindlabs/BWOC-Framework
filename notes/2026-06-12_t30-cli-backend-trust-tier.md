---
title: Phase 6 t30 (PHASE6-2) — cli-backend trust tier & ambient-backend refusal
date: 2026-06-12
tags:
  - type/note
  - area/harness
  - area/security
  - phase/6
---

# Phase 6 t30 — the `cli` backend is *ambient*; close the #271 hole it opens

## Finding (the dukkha)

The `cli` backend (`provider/cli.rs`, #277) is a **provider transport**, not a
tool: it shells out to `<cli> -p --model … --output-format json` and returns the
text. It is **chat-only by design** — the harness `tools` parameter is ignored,
because the vendor CLI runs *its own* tools internally.

That relocates tool execution **outside** the harness. The Phase 5 stack — the
`policy` capability gate (#271: an Untrusted turn is effectively read-only), the
per-turn FS jail, the egress filter — all gate **harness** tool calls. On the
cli backend there are none to gate; the vendor subprocess acts with full ambient
authority. So the #271 guarantee is not *weakened* on this backend, it is
**structurally unenforceable**.

The live blast radius: `bwoc-agent`'s gateway auto-process
(`autoprocess.rs`, 2026-06-09) feeds **Untrusted** internet-sourced messages to
`bwoc-harness --chat --backend <manifest.backend>` and auto-denies every
permission request. If the manifest's backend is `cli`, all of that is moot —
remote untrusted input reaches an unconfined, tool-capable subprocess.

## Ruling (agent-luban)

> **#271 is a hard guarantee only where the harness owns the tool-execution
> boundary.** On an ambient backend it cannot be honored, so the response splits
> by trust of the principal driving the turn:
>
> - **Untrusted / automated path (gateway auto-process):** HARD → **refuse**.
>   There is no safe way to honor "untrusted = read-only" on a backend that
>   can't enforce it; fail closed (saṃvara — don't open a door you can't guard).
> - **Interactive operator (a Trusted `LocalOperator`):** informed choice →
>   **allow + LOUD-warn**, and **surface the degraded tier** so the operator and
>   any peer deciding whether to trust the agent can see it.

This is "Option A". It is not a global hard-vs-soft binary — it is *hard in the
untrusted path, informed-choice in the trusted path*.

## What changed

- **`bwoc-core::trust`** (single source of truth) — new `BackendTrust
  { Confined, Ambient }` + `backend_trust_tier(&str)`. Known-ambient allowlist:
  only `"cli"` is `Ambient`; everything else (incl. unknown backends, which route
  to the HTTP client) is `Confined` — fail-*safe*. A doc-contract + the test
  `cli_is_the_only_ambient_backend` require any future tools-escape backend to be
  added here.
- **`bwoc-agent::autoprocess`** — `AutoProcessor` gained `ambient_backend`
  (computed in `detect`). `is_active()` now also requires `!ambient_backend`, so
  an ambient-backend agent **never auto-processes** untrusted remote input
  (fail-closed at the single chokepoint, `main.rs` `if … is_active()`).
  `announce()` prints a LOUD refusal explaining why.
- **`bwoc-harness::main::build_provider`** — the `"cli"` arm emits a LOUD stderr
  SECURITY warning (confinement & #271 do not apply) before constructing the
  client. Informs the interactive operator every run.
- **`bwoc-cli` surfaces** — `bwoc status <agent>` detail prints a `trust:` line
  (`⚠ AMBIENT …` / `confined …`); `bwoc status` and `bwoc list` tables print a
  footer counting ambient agents; both `--json` outputs carry a `trust_tier`
  field. Tier is **derived** from the backend via `backend_trust_tier`, never
  stored (Yoniso Manasikāra — no field to drift).

## Decisions / alternatives

- **Derive, don't store.** No `hostTrustTier` manifest field — it could drift
  from the actual backend. The tier is a pure function of the backend string.
- **Allowlist `cli`, default Confined.** The danger is specific to backends that
  relocate tool execution; only `cli` does. Defaulting unknown→Ambient would
  break every normal HTTP backend's auto-process for no security gain (unknown
  backends route to the harness-confined HTTP client). The cost is a coupling:
  a new tools-escape backend must be added to the `Ambient` arm — pinned by test
  + doc-contract, and it lives right next to `build_provider`'s backend match.
- **Refuse at `is_active()`, not inside `handle()`.** Single chokepoint; the
  whole loop never engages. `handle()` has one call path (under the gate), so a
  redundant guard there would be over-engineering (Mattaññutā).
- **Footer note, not a table column.** The fixed-width BACKEND column already
  shows `cli`; a width-safe footer + the detail `trust:` line + JSON field cover
  status/list without column surgery.

## Out of scope (named)

- The interactive cli session still *runs* (informed-choice) — we warn, we do
  not block. Blocking trusted operator use would punish the legitimate
  subscription-auth use case #277 exists for.
- `cli` remains chat-only; no attempt to lift tool-calling into it.

## Proof

`bwoc-core`: `cli_is_the_only_ambient_backend`. `bwoc-agent`:
`ambient_backend_refuses_auto_process`, `confined_backend_is_not_flagged_ambient`.
Workspace gates: fmt clean, clippy `-D warnings` clean, `cargo test` green
(see end-of-session summary for counts).

## Related

- `notes/2026-06-09_agent-untrusted-autoprocess.md` — the auto-process loop hardened here.
- `notes/2026-06-10_cli-backend-subscription-auth.md` — the cli backend (#277).
- `crates/bwoc-harness/src/policy/mod.rs` — the #271 capability gate this protects.
