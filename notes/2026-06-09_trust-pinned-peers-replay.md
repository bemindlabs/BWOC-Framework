# 2026-06-09 — Trust gate: pinned peers + replay defense

Two security gaps closed so a **standalone agent** can safely receive gateway-relayed messages from the open internet (part of the approved standalone-agent plan, Component 3). Both live in `crates/bwoc-agent/src/trust.rs`; no `main.rs` change (the replay state is interior-mutable in `TrustContext`).

## What changed

- **Pinned-peer keyring (`.bwoc/peers.toml`).** A remote sender that no workspace registry can resolve (the standalone / gateway case) is now verified against a **locally-pinned ed25519 public key**. `evaluate`'s sender resolution gained a third source after the local registry and `routes.toml` peer: `resolve_pinned_peer` reads `[[peer]] id, pubkey, declares?` and synthesizes a minimal sender manifest via the new `Manifest::pinned_peer` (bwoc-core). It works **with no workspace at all**, so the old immediate `no_workspace` refusal no longer blocks a standalone agent. A pinned sender is flagged `cross_workspace` → must carry a provable signature, verified against the pinned key by the existing `verify_signature`.
- **Replay defense (`ReplayGuard`).** For **cross-workspace/gateway** senders only, a bounded seen-`(from, nonce)` set rejects duplicate envelopes and an ISO-8601 `ts` freshness window (±5 min / +1 min skew, lexicographic on the first 19 chars — no date crate) rejects stale/future ones. New refusal reasons: `replayed`, `stale_replay`, `future_ts`. The guard is `Mutex<ReplayGuard>` on `TrustContext` (the serve loop is single-threaded but `evaluate` takes `&self`).

## Decisions

- **Pinning, not TOFU.** Trust-on-pin (the operator adds the key) over trust-on-first-use: an internet-facing ingress with TOFU lets the first attacker mint an identity. TOFU is left as a future opt-in.
- **Replay scoped to remote senders.** Local delivery is fresh and trusted and the relay-replay threat is the cross-machine one; scoping avoids any impact on local message flows.
- **`no_workspace` vs `unknown_sender`.** `no_workspace` now fires only when there is genuinely nowhere to look (no workspace AND no `peers.toml`); a present keyring with the sender absent is the more accurate `unknown_sender`.
- **`ts` window without a date dep.** Reused `bwoc_core::time::format_iso8601` to render the cutoff strings and compared lexicographically — dependency-quarantine intact.

## Bugs surfaced

- The existing `cross_workspace_signed_sender_verifies_via_routes` test used a fixed 2026-05-27 `ts`; the new freshness window correctly rejected it as stale. Updated the valid case to a current `ts` (the stale path has its own test now).

## Status / deferred

- Done: pinned-peer verification + replay/freshness. Still in the plan: the gateway recv binary must **drop any inbound `from = "user"`** envelope (a remote message must never present the local-trusted principal — handled at the recv ingress, separate gateway-repo change), and the **untrusted auto-process** wiring (gateway turns run read-only / fail-closed inside the Phase 5 jail).
- End-state (deferred): workspace-qualified ids, key rotation/revocation, TOFU opt-in, gateway identity-hint.

## Related

- `crates/bwoc-core/src/manifest.rs` — new `Manifest::pinned_peer` constructor.
- `bwoc-gateway` `bwoc-gateway-recv` (the transport that feeds these envelopes).
- Plan: `~/.claude/plans/inherited-strolling-porcupine.md`.
