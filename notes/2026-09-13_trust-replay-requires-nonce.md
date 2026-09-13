# 2026-09-13 — Verified envelopes must be replay-checkable

Found while triaging the open CodeQL alerts. Alerts #24/#25 flag the `unwrap_or("")` defaults in `bwoc-agent/src/trust.rs` as "hard-coded cryptographic value". As a crypto finding that is a false positive: the empty string is just a default. Reading the code behind it turned up a real replay hole.

## What changed
- In `evaluate`, on the cross-workspace / gateway path: if the signature verified (`verified_from.is_some()`) but `nonce` or `ts` is empty, the envelope is refused with reason `unreplayable`.

## Why
- `ReplayGuard::check` returns `None` (accept) for an empty nonce without recording anything, and `ts_outside_window` accepts an empty ts.
- `canonical_bytes` signs whatever the fields contain, empty strings included. So a peer's key could produce a nonce-less, ts-less envelope that verifies. After that, anyone who captured it could re-deliver it indefinitely: it keeps passing, and each delivery is attributed to the verified identity.

## Decisions
- **Refuse only verified envelopes.** An unsigned envelope proves no identity, so replaying it gains nothing a forger doesn't already have. Refusing those too would break warn-mode peers for no benefit.
- **Compatible with 2.x / 3.0.** The only production signer, `bwoc send` (`send.rs:319-330`), always sets `nonce` together with `sig`, and always sets `ts`.

## Tests
- `verified_envelope_without_nonce_or_ts_is_refused`:
  - no nonce refuses, and a re-delivery also refuses
  - no ts refuses
  - the normal shape still passes

## Related
- CodeQL alerts #24, #25
- `notes/2026-06-09_trust-pinned-peers-replay.md`
