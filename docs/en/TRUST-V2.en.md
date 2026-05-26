---
title: Trust v2 — Cryptographic Message Authentication
aliases:
  - Message Signing
  - Identity Signing
  - HV2-4
tags:
  - group/harness
  - type/spec
  - meta/security
status: draft (spec gate — HV2-4 / BWOC-3, awaiting maintainer ratification)
canonical-source: Musāvāda-veramaṇī (4th precept) + Kalyāṇamitta 7 (AN 7.36)
---

# Trust v2 — Cryptographic Message Authentication

| | |
|---|---|
| **Document** | docs/en/TRUST-V2.en.md |
| **Bilingual Pair** | docs/th/TRUST-V2.th.md |
| **Workstream** | HV2-4 (BWOC-3), gated by GH #39; unblocks #20 (cross-workspace give-feedback) |
| **Primary Framework** | Musāvāda-veramaṇī — abstaining from false speech (no forged identity) |
| **Supporting** | Kalyāṇamitta 7 — the trust this layer authenticates |
| **Status** | **Spec gate.** No crate code until this spec is ratified. |

---

## 0. Scope and the "Trust v2" name

> [!warning] Two layers share the word "trust" — keep them distinct.
> `interconnect/trust.md` already ships a feature called **"Trust v2" (warn-mode, v2026.5.24)**: the **Kalyāṇamitta-7** model — seven self-declared booleans by which a recipient decides *whether it wants messages from a sender at all*. That is **authorization**: *should I listen to this peer?*
>
> This document specifies a **different, lower** layer: **authentication** — *is this message actually from the agent it claims to be from?* The Kalyāṇamitta-7 decision is only sound if the sender's identity is real; today it is asserted, not proven.

| Layer | Question | Mechanism | Where |
|---|---|---|---|
| Authorization (shipped) | Do I accept messages from this *kind* of peer? | Kalyāṇamitta-7 booleans + `requiredTrust` | `interconnect/trust.md` |
| **Authentication (this spec)** | Is this message genuinely from that peer? | **ed25519 signed envelopes** | harness inbox + guardrails |

Because the name collides, **§9 asks the maintainer to ratify the filename** (keep `TRUST-V2` with this layering, or rename to e.g. `SIGNING.en.md` / `MESSAGE-AUTH.en.md`).

---

## 1. Threat addressed

> An agent forges the `sender` of an inbox message to impersonate a trusted peer and bypass the recipient's Kalyāṇamitta-7 refusal rules.

Today, identity is checked by `check_identity_spoof()` (`crates/bwoc-harness/src/policy/guardrails.rs:293`, invoked at `:81`) — a **keyword/substring heuristic**. It catches a clumsy `"I am agent-X"` string but cannot prove authorship, and is trivially evaded by rephrasing. There is no cryptographic binding between a message and its claimed sender.

This maps to **Musāvāda** (false speech): a forged identity is the system-level lie. The remedy is to make truthful authorship *checkable*, so a lie is detectable rather than merely discouraged.

---

## 2. Why ed25519, not HMAC

A shared-secret MAC (HMAC) fails the **cross-workspace** case that motivates the work (#20, give-feedback across a trust boundary):

- A shared secret must be distributed to every party that needs to *verify*. Any holder of the verify key can also *forge*, because the same secret signs and checks.
- Across a workspace/trust boundary there is no safe channel to share such a secret, and one leaked copy forges for everyone.

**ed25519** (asymmetric) separates the two: the **private** key only signs, the **public** key only verifies. Publishing the public key lets anyone verify and *no one* forge. ed25519 is small (32-byte keys, 64-byte signatures), fast, and has no parameter footguns.

> [!note] Dependency note (dep-quarantine)
> ed25519 needs a signing crate (candidate: `ed25519-dalek`). Heavy/crypto deps live **only** in `bwoc-harness` per the dep-quarantine, alongside `keyring`/`landlock`. This is consistent with the existing rule; the exact crate + version is a §9 ratification item.

---

## 3. Keypair lifecycle

| Stage | What | Where |
|---|---|---|
| Generation | One ed25519 keypair per agent, at incarnation. | `bwoc new` / incarnation. |
| Private key | Stored in the OS keyring via the existing broker (`crates/bwoc-harness/src/tools/auth.rs`, `CredentialRequest { keyring_service: "bwoc/signing", keyring_account: <agentId> }`). Never written to disk, never in telemetry. | OS keyring. |
| Public key | Published with the agent's identity so recipients can verify. | Agent manifest (`config.manifest.json`) and/or the agent's `interconnect/trust.md` descriptor. **(§9: which is canonical.)** |
| Rotation | Replace keypair; old public key retired. | `bwoc` subcommand **(§9: rotation UX deferred?)**. |

> [!warning] The note's `interconnect/shared.toml` does not exist.
> The planning note cited `interconnect/shared.toml` as the publication seam; the real per-agent trust descriptor is **`interconnect/trust.md`** (already bilingual, already carrying the Kalyāṇamitta-7 declaration). Public-key publication should extend that existing descriptor or the manifest — not a new file.

---

## 4. Signed envelope

Every inter-agent message is wrapped in a signed envelope. The signature covers a canonical serialization of all fields except `sig`.

```jsonc
{
  "sender":    "agent-luban",        // claimed author (the public key to verify against)
  "recipient": "agent-erlang",       // bound: a captured envelope can't be replayed to a third party
  "nonce":     "b3f1…",              // unique per (sender, recipient); replay guard
  "ts":        "2026-05-26T10:00:00Z", // issuance time; bounds the replay window
  "payload":   { /* the message body */ },
  "sig":       "ed25519(sender_priv, canonical({sender,recipient,nonce,ts,payload}))"
}
```

- **`recipient` is signed** so a valid envelope captured in transit cannot be re-aimed at another agent.
- **`nonce` + `ts`** together defeat replay (see §5).
- Canonical serialization (field order, encoding) must be fixed and identical on both sides. **(§9: pick the canonical form — sorted-key JSON vs a length-prefixed binary concat.)**

---

## 5. Verification + replay window

On receipt, before the Kalyāṇamitta-7 authorization check, the recipient:

1. Resolves `sender`'s public key (unknown sender → refuse).
2. Verifies `sig` over `canonical({sender,recipient,nonce,ts,payload})` (fail → refuse).
3. Checks `recipient` is *this* agent (mismatch → refuse).
4. Checks `ts` is within `±skew` of now (default **±5 min**, §9) (stale/future → refuse).
5. Checks `nonce` is unseen within the sliding window for this `sender` (seen → refuse as replay).
6. Records the nonce; only then hands the payload to the Kalyāṇamitta-7 layer.

**Sliding nonce window:** the recipient keeps recently-seen `(sender, nonce)` pairs with their `ts`, evicting entries older than the skew bound. Memory is bounded by `skew × message-rate`. This *replaces* the keyword `check_identity_spoof()` heuristic.

> [!note] The audit log is a **new** artifact, not an existing seam.
> The planning note referred to `inbox.refusals.jsonl` "as the seam" — it does **not** exist today (no `refusals` anywhere in the harness). This spec **proposes** an append-only refusal/audit log (e.g. `inbox.refusals.jsonl`) recording each refused envelope `{ts, sender, recipient, reason}` for observability. It is part of this workstream, not a pre-existing hook.

---

## 6. Migration from `check_identity_spoof`

- Verification (§5 steps 1–5) supersedes `check_identity_spoof()` (`guardrails.rs:293`).
- **Warn → enforce rollout**, mirroring how Kalyāṇamitta-7 shipped warn-mode first: unsigned or unverifiable messages **warn** (and log) initially, then **refuse** once agents have published keys. A `vetted_mode`-style enum (`off` / `warn` / `enforce`) keeps backward compatibility during the transition.
- Messages from agents with no published key: governed by the rollout mode (warn-mode accepts with a log; enforce refuses).

---

## 7. Buddhist grounding

| Principle | Mapping |
|---|---|
| **Musāvāda-veramaṇī** (4th precept) | A forged identity is the system's false speech. Signatures make authorship truthful and *checkable* — abstaining from the lie by construction. |
| **Kalyāṇamitta 7** (AN 7.36) | Admirable-friend trust is only meaningful between *authenticated* friends. This layer guarantees the friend at the door is who they claim, so the existing trust booleans mean what they say. |
| **Sīla (guardrail invariant)** | Verification runs in the guardrail path (`run_pipeline`) before authorization — the safety pipeline stays the single gate; this strengthens it, adds no bypass. |

---

## 8. Out of scope (this spec)

- Payload **encryption** (confidentiality). This spec is authentication/integrity only; messages are signed, not encrypted.
- A PKI / certificate authority. Keys are published with agent identity (trust-on-publication), not chained to a root.
- Key rotation UX and revocation lists (sketched in §3; depth deferred — §9).

---

## 9. Open decisions for maintainer ratification

The gate. Each must be confirmed before any crate code:

1. **Filename / name.** Keep `TRUST-V2` (with §0 layering) or rename to `SIGNING` / `MESSAGE-AUTH` to avoid the Kalyāṇamitta-7 "Trust v2" collision?
2. **Canonical public-key home.** Agent manifest (`config.manifest.json`), the `interconnect/trust.md` descriptor, or both (one canonical, one mirror)?
3. **Signing crate + version** under dep-quarantine (`ed25519-dalek`?).
4. **Canonical serialization** for the signed bytes (sorted-key JSON vs binary concat).
5. **Replay window**: skew bound (default ±5 min?) and nonce-window eviction policy.
6. **Rollout**: confirm warn→enforce staging and the mode enum's default.
7. **Refusal/audit log**: confirm the new `inbox.refusals.jsonl` artifact + its schema.
8. **Key rotation/revocation**: in-scope for HV2-4 or a follow-up?

---

## 10. Related

- `interconnect/trust.md` — Kalyāṇamitta-7 authorization (the layer above).
- `modules/agent-template/docs/en/THREAT-MODEL.en.md` — T-category this closes (identity spoofing).
- `crates/bwoc-harness/src/policy/guardrails.rs:293` — `check_identity_spoof` (replaced).
- `crates/bwoc-harness/src/tools/auth.rs` — keyring broker (private-key storage).
- `notes/2026-05-25_harness-v2-planning.md` — HV2-4 decision. GH #39, #20.
