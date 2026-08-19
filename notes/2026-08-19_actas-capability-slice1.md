# 2026-08-19 — Act-as-user capability tier (L3 middle tier, Slice 1)

The first, **inert** slice of the L3 middle trust tier (#452): the pure
capability-layer machinery for act-as-authenticated-user, gated behind a
`None` actor so it is **zero production behavior change**. Unblocks the L3
inbound-service loop (the "first real third consumer" the #452 ADR waited for).
Preceded by a design-validation + red-team workflow (all 3 lenses held) and
two architect decisions.

## Architect decisions (this session)

- **Out-of-band session-actor** (over in-band history read): the verified
  identity is a daemon-set, per-session immutable fact, never read from message
  content — so nothing on the message channel can forge it. Closes the one HIGH
  the red-team found.
- **Explicit peer pin** for the v1 allowlist (over "any verified sender"):
  act-as-user authority is opt-in per peer. (Enforced at minting, Slice 4.)

## Key design win over the ADR

The ADR proposed a new `Principal::AuthenticatedUser`. The map showed
`Principal::A2aSender { from, verified }` **already exists**, already resolves
`Untrusted`, and is already pinned by `exactly_two_principals_are_trusted` with
zero production construction sites. So the mid-tier **reuses A2aSender** — **net
`trust.rs` edit = zero**, the monotonic latch and the two-trusted invariant are
untouched. The genuinely-new concept lives entirely at the **capability** layer.

## What changed (Slice 1 — inert)

- `policy/mod.rs`: new `Capability::ActAsUser { to }` between `WorktreeWrite`
  and `Gated`; `classify_capability` maps `bwoc_send` → `ActAsUser` carrying its
  `to` reply-target (structural — no identity here). `run_pipeline` gains a
  **required** `authenticated_actor: Option<&str>` param (a forgotten thread is a
  compile error, mirroring `turn_trust`). The new gate arm authorizes an
  Untrusted-turn `bwoc_send` ONLY when `(Some(actor), Some(to)) && actor == to` —
  a verified sender may reply to itself and no one else; every other shape denies
  exactly like `Gated`.
- `session_trust.rs`: new `pub struct AuthenticatedActor { id }` and
  `pub fn scan_authenticated_actor(history, actor) -> Option<String>` — fail-closed,
  reusing `taints_turn` for parity with the trust scan. Grants `Some(id)` only
  when the actor is `Some` **and** every taint-bearing ingress is the matching
  `A2aSender{from==id,verified:true}` with at least one present; ANY foreign taint
  (tool output, second/unverified sender, tainted `Summary`, `Unknown`) → `None`.
- `execute.rs`: the ONE production call site passes `None` → the ActAsUser arm
  always denies in production → `bwoc_send` stays denied on untrusted turns,
  exactly as before. `scan_authenticated_actor` is pub + fully unit-tested but
  wired to the daemon only in Slice 4.

## Why inert-first (Yoniso ordering)

Prove the gate green before feeding it identity. Slice 1 lands the fail-closed
trust logic for isolated review with zero risk; the highest-risk surface (daemon
minting + the ingest/checkpoint forgery clamp + the from-spoof executor fix) is
sequenced LAST (Slice 4), after the plumbing slices 2–3.

## Tests

- `act_as_user_gate_requires_matching_authenticated_actor` — the gate decision
  table: no-actor→denied, matching→Proceed, mismatched→denied, `to`-less→denied,
  trusted-turn→no-op Proceed.
- 6 `scan_authenticated_actor` tests pinning the fail-closed taint rule
  (no-actor, matching+trusted-ingress, absent-sender, unverified/mismatched,
  every foreign-taint class evaporates, multi-message-same-sender allowed).
- Golden capability table updated: `bwoc_send` → `ActAsUser` (with + without `to`).

## Grounded in an adversarial review

Two workflows: a design-validation + red-team (Map → Design → RedTeam →
Synthesize) that produced this plan — all 3 red-team lenses (forgery,
taint-launder, invariant-blast) **held** — then a code review of the actual
Slice-1 diff (**3 raised → 0 confirmed** after adversarial refutation). The 3
raised were all low, all refuted as non-defects: the model-visible deny *reason*
string changed for `bwoc_send` (the decision — denied — did not), and two
test-coverage suggestions. Both coverage suggestions were folded in anyway (they
pin real corners cheaply): a **clean** `Summary` alongside the sender must still
yield `Some` (dual of the tainted-summary case), and empty-history + a set actor
→ `None` (the fail-closed root).

## Status / deferred (the remaining slices)

- **Slice 2** — ingest + checkpoint forgery clamp (`A2aSender → Unknown`),
  defense-in-depth, harmless today.
- **Slice 3** — extend `evaluate`/`TrustOutcome` (bwoc-agent) to carry the
  verified `{from}`; `verified:true` only from the `enforce=true` crypto-success
  branch.
- **Slice 4** — daemon minting + out-of-band session-actor + from-spoof executor
  fix (force `from=<self_id>`; the Musāvāda guardrail is a substring toy) +
  peer-pin allowlist. Activates the feature (highest risk, lands last).

## Related

- Design ADR: issue #452. Spec: `docs/en/LOOP-ENGINEERING.en.md` (L3 middle tier).
- Reuses: `Principal::A2aSender` (`bwoc-core/trust.rs`), `taints_turn`
  (`session_trust.rs`).
