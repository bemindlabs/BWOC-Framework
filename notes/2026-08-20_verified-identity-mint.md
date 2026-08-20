# 2026-08-20 — The verified-identity mint (#452 Slice 3)

Plumbing slice: `trust::evaluate` now reports **which sender's signature actually
verified**, so a later slice can turn that into act-as-user authority. No
consumer yet, by design — the mint and its consumer are deliberately reviewed
separately, because the consumer is the riskiest surface in the whole tier.

## The trap this slice exists to avoid

The red-team's top hardening was *"mint `verified: true` ONLY inside the
crypto-success branch — never off a bare `Pass`."* Reading the code showed why
that warning was load-bearing:

`verify_signature` returned `Option<TrustOutcome>`, and **`None` meant two
completely different things**:

```rust
Ok(())                   => None,   // crypto verified — identity PROVEN
Err(_) /* !enforce */    => None,   // key unloadable — proved NOTHING
_                        => None,   // no sig / no pubkey, warn mode — proved NOTHING
```

Minting off "the call returned `None`" would therefore have handed an identity
to senders that presented no proof at all. `evaluate` has the same shape at a
higher level: **four** of its passing paths never run crypto —

1. signing off + Kalyāṇamitta gate inert (nothing runs),
2. a malformed envelope (never parsed),
3. `from == "user"` (returns *before* any signature check — and it is
   **attacker-selectable**: any envelope may simply claim it),
4. an unenforced mode where an unverifiable signature still proceeds.

## What changed

- **`SignatureCheck` replaces `Option<TrustOutcome>`** — `Verified` /
  `Unproven` / `Refused`. The conflation is gone at the type level; the one arm
  that may mint is `Ok(()) => Verified`.
- **`TrustOutcome::Pass` is now a struct variant** `Pass { verified_from:
  Option<String> }`. This is the enforcement: a bare `Pass` no longer compiles,
  so every existing path *and any added later* must state explicitly whether it
  proved an identity. The compiler listed all six construction sites; each was
  decided individually.
- **`evaluate` mints only for a cross-workspace sender whose crypto verified** —
  the ratified v1 identity set. A same-workspace sender passes *without*
  minting: a local agent is a different trust context, and widening the set is a
  deliberate later decision, not a side effect.

## Mutation testing found a real hole in my own tests

Three mutations were run against the finished slice:

| Mutation | Caught? |
|---|---|
| `from == "user"` mints its claimed id | ✅ red |
| `Unproven` counted as verified (cross-workspace arm) | ⚠️ green — **but correctly so**: with `sig` required and `enforce = true`, `verify_signature` cannot return `Unproven` on that path, so the mutation is semantically dead code. Verified by enumerating the match arms. |
| the **local** branch mints too | ❌ **green — a real gap** |

The third was a genuine miss: my "local senders don't mint" decision was
enforced by nothing. Added
`a_verified_same_workspace_sender_passes_without_minting`, which builds a real
same-workspace signed sender end-to-end; the mutation is now caught.

The mint-discrimination test covers all four unverified paths, including the
sharpest case — `from == "user"` under `SigningMode::Enforce`, which *looks*
like the strictest posture yet proves nothing.

## Status

54 bwoc-agent tests pass; `cargo test --workspace` clean; `clippy --workspace
--all-targets -D warnings` clean. The field carries `#[allow(dead_code, reason =
…)]` because a plumbing slice has no consumer — stated openly rather than papered
over with a speculative caller.

## Related

- Slice 1 (`ActAsUser` tier, inert): `notes/2026-08-19_actas-capability-slice1.md`
- Slice 2 (forgery clamps + the live provider escalation):
  `notes/2026-08-20_principal-forgery-clamp.md`
- Remaining — **Slice 4** (high risk): daemon minting via
  `ChatMessage::verified_a2a_sender`, the out-of-band session actor, forcing
  `from=<self_id>` on the reply, and the peer-pin allowlist. ADR: issue #452.
