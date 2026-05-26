# 2026-05-26 — HV2-4 Trust v2 signing spec (BWOC-3, spec gate)

Drafted the gated `TRUST-V2.en.md` + `.th.md` spec for ed25519 message authentication. **Spec only — no crate code**, by the planning-note gate ("gate on its own TRUST-V2 spec pass before build") and the parked #6 Trust-v2 decision. Parked at the maintainer-ratification gate (§9). Done in the auto-pilot batch but, unlike BWOC-5/6/4, deliberately stops before implementation.

## What changed

- `docs/en/TRUST-V2.en.md` + `docs/th/TRUST-V2.th.md` (bilingual pair, 11 sections each, parity verified).
- This note. **No code.**

## Decisions (proposed in the spec, for ratification — not finalized)

- **Authentication ≠ authorization — two layers.** The spec positions ed25519 signing as the *authentication* layer (is this message really from that agent?) **beneath** the already-shipped Kalyāṇamitta-7 *authorization* layer (do I accept this kind of peer?). Verification runs in the guardrail path before the trust-boolean check.
- **ed25519 over HMAC.** A shared-secret MAC can't cross a workspace trust boundary (any verifier can forge). Asymmetric keys separate sign (private, in keyring) from verify (public, published). *Musāvāda — make authorship truthful and checkable.*
- **Warn→enforce rollout**, mirroring how Kalyāṇamitta-7 shipped warn-mode first; a `vetted_mode`-style enum keeps backward compat.

## Corrections to the planning note (Yoniso manasikāra — verified against the tree)

- **`interconnect/shared.toml` does not exist.** The real per-agent trust descriptor is `interconnect/trust.md` (already bilingual, already carries the Kalyāṇamitta-7 declaration). Public-key publication should extend the manifest / `trust.md`, not a phantom file.
- **`inbox.refusals.jsonl` does not exist** (no `refusals` anywhere in the harness). It is *proposed new* in this spec (an append-only refusal/audit log), not an existing seam.
- **"Trust v2" name collision.** `interconnect/trust.md` already calls the Kalyāṇamitta-7 feature "Trust v2 (warn-mode, v2026.5.24)". Naming this signing spec `TRUST-V2` collides. Flagged in §0 and raised as ratification item §9.1 (keep with explicit layering, or rename to `SIGNING`/`MESSAGE-AUTH`).
- Verified real seams: `check_identity_spoof()` (`guardrails.rs:293`/`:81`), keyring broker (`tools/auth.rs` `CredentialRequest`).

## Status / deferred

- **Parked at the spec gate.** BWOC-3 left `in_progress` (spec drafted, build blocked pending §9 ratification) — deliberately *not* `review`, since no implementation exists yet.
- §9 lists 8 open decisions the maintainer must confirm before any crate code (filename, public-key home, signing crate, canonical serialization, replay window, rollout mode, audit-log schema, key rotation scope).
- Out of scope here: payload encryption, PKI/CA, revocation lists.

## Related (links)

- `docs/en/TRUST-V2.en.md` · `docs/th/TRUST-V2.th.md` (the spec).
- `interconnect/trust.md` — the Kalyāṇamitta-7 authorization layer above.
- `notes/2026-05-25_harness-v2-planning.md` — HV2-4 decision. GH #39, #20, #6 (parked Trust-v2 decision).
- Sibling built workstreams: `notes/2026-05-26_hv2-2-durable-runs.md`, `hv2-3-run-end-retrospective.md`, `hv2-1-sangha-runtime.md`.
