# 2026-05-25 — Trust v2: signed envelopes & cross-workspace identity (design draft)

Design draft for the keystone deferred decision (#17 / #39 HV2-4) that gates **#20 give-feedback** and all cross-workspace *write* paths. Drafted by agent-oracle (architect lane) for maintainer review.

> **SUPERSEDED-IN-PART (2026-05-25).** After drafting this I found **PR #40** already implements HV2-4 (ed25519 signed envelopes) — a Yoniso miss on my part (I didn't check open PRs first). #40 settles three of the four axes below: **identity** (per-agent key) and **backward compat** (`requireSignature=false` default) match this draft; **crypto** uses the `ed25519-dalek` crate (this draft leaned toward an `ssh-keygen` shell-out — revised: the in-process crate is the stronger per-message choice; recommend accepting it behind a `--features signing` gate). The still-useful part of this draft is the **key-distribution** discussion (§3): #40 puts the pubkey in the manifest; a `routes.toml` TOFU/pin layer is the cross-workspace hardening, addable later without touching the envelope. See the #39 review comment.

## Dukkha — what's actually unsafe today

v1 trust works *within* a workspace: `bwoc send --from <agent>` stamps a sender id, the recipient daemon resolves that id **in its own registry** and evaluates the sender's declared Kalyāṇamitta-7 qualities (gate modes `off`/`warn`/`refuse`). Cross-workspace, this collapses: routing (Track A) can *deliver* to a peer, but the envelope's `from` is a **bare, unforgeable-only-by-convention string** (`agent-oracle`). A peer workspace can stamp `--from agent-oracle` and the recipient cannot tell a real Oracle from an impostor. Correct current behavior: a cross-workspace sender resolves as `unknown_sender` → refused. That safe default is *why* give-feedback (a write/influence path) is blocked — there is no provable origin to trust.

## Nirodha — success state

A recipient can **cryptographically verify** that an envelope claiming `from = agent-oracle@peer-ws` was authored by that agent in that workspace, **without any central authority** (Anattā / SN 22.59 — no central self, so no central CA), with every peer equal (Samānattatā), and with v1 unsigned flows still working unchanged (backward compatible).

## Magga — the four design axes (each with options)

### 1. Identity shape — **[DECIDE]**
Promote the bare `from` to **workspace-qualified**: `agent-oracle@peer-ws`. The seam was already marked in `routing.md`. The `@workspace` part must map to something verifiable.

- **(a) Per-agent keypair** — each agent owns a signing key; identity = `agent@workspace` bound to that agent's public key. Finest granularity; matches "agent is the trust unit" in v1.
- **(b) Per-workspace keypair** — one key signs for the whole workspace; `@workspace` is the trust anchor, agent id is a claim *inside* the signed payload. Fewer keys to manage; weaker per-agent isolation (a compromised workspace key forges any of its agents).

→ *Lean toward (a)* — v1 already treats the agent as the trust unit (per-agent `requiredTrust`); per-workspace would be a coarser model that contradicts it.

### 2. Crypto + tooling — **[DECIDE]** (dep-lean tension)
The codebase prefers **shell-out over crates** (git via shell, no `git2`; `gh`/`curl` behind a `CommandRunner` seam — see #8).

- **(a) Shell-out to `ssh-keygen -Y sign|verify`** (OpenSSH signatures, allowed-signers file). Zero new Rust crypto deps; every dev already has it; matches the established shell-out style and the `CommandRunner` test seam. Cost: process spawn per verify; ssh-keygen ergonomics.
- **(b) `minisign` / `signify` shell-out** — purpose-built signing, tiny, but a new external binary users must install (fails the "every dev has it" test).
- **(c) `ed25519-dalek` crate** — in-process, fast, no spawn; but a new crypto dependency in `bwoc-core` (cuts against dep-lean, adds an auditable surface).

→ *Lean toward (a)* — consistent with the project's "shell-out, mockable runner" pattern; keeps `bwoc-core` crypto-dep-free. Revisit (c) only if per-message spawn cost shows up in telemetry.

### 3. Key distribution WITHOUT a central authority — the hard part — **[DECIDE]**
This is where Anattā bites: no key server, no CA. Options, in increasing trust strength:

- **(a) TOFU + pin in `routes.toml`** — the peer publishes its public key at a well-known path in its workspace (e.g. `.bwoc/identity/<agent>.pub`, readable over the existing local-FS route). On first contact the recipient records the key; the operator may **pin** the expected fingerprint in `routes.toml` next to the route. Mismatch later → refuse. Mirrors SSH known_hosts. Lowest ceremony; local-FS-friendly (matches routing v1's local-FS-only scope).
- **(b) Operator-pinned only (no TOFU)** — the route entry *must* carry the peer's pubkey fingerprint or it won't verify. Strongest, most friction; no first-use window.
- **(c) Web-of-trust later** — peers vouch for peers' keys (Kalyāṇamitta as literal key-signing). Powerful, premature for v1.

→ *Lean toward (a)* with the **pin optional but recommended**: TOFU keeps the local-FS mesh frictionless; the pin closes the first-use gap when an operator cares. Network transport (ssh/http key fetch) stays deferred with routing's network transport.

### 4. Gate integration + backward compat
- Envelope grows a `sig` block: `{ alg, signer: agent@workspace, sig }` over a canonical serialization of the envelope body. Additive — v1 readers ignore it.
- New per-recipient knob `trust.requireSigned` (default **false**). Decision table at inbox poll:
  - unsigned + `requireSigned=false` → **v1 behavior exactly** (no regression).
  - unsigned + `requireSigned=true` → refuse (`unsigned_envelope`).
  - signed + verify-fail → refuse (`bad_signature`) regardless of mode.
  - signed + verify-ok → resolve signer's qualities, apply existing `off`/`warn`/`refuse`.
- **give-feedback unlocks** precisely when a cross-workspace envelope is signed + verified + the recipient's gate passes — i.e. it rides this machinery, no separate trust path.

## Decisions / recommendations summary

| Axis | Options | Oracle lean |
|---|---|---|
| Identity | per-agent / per-workspace key | **per-agent** |
| Crypto | `ssh-keygen -Y` / minisign / dalek crate | **`ssh-keygen -Y` shell-out** |
| Key dist | TOFU+pin / pin-only / web-of-trust | **TOFU + optional pin** |
| Compat | `requireSigned` default | **false (opt-in)** |

## Alternatives considered (rejected)

- **Central key registry in the workspace root** — simplest, but reintroduces a central authority → violates Anattā / the no-central-broker invariant that routing.md is built on.
- **Reuse the existing `bwoc check` evidence model for cross-workspace** — v1 `check` verifies *self-declared* manifest evidence; it proves nothing about *who sent a message*. Different problem.

## Status / deferred / sequencing

- **Not implemented.** This is a design for review. Network transport (remote key fetch) stays deferred with routing's ssh/http transport — v1 signing is **local-FS mesh only**, consistent with routing v1.
- If approved, the natural build order: (1) identity-qualified `from` + envelope `sig` block (additive, no behavior change at `requireSigned=false`) → (2) `ssh-keygen` signer/verifier behind a `CommandRunner` seam + `.bwoc/identity/` key publication → (3) `routes.toml` pin + TOFU store → (4) flip give-feedback (#20) on. Each step ships behind `requireSigned=false`, so nothing regresses until an operator opts in.
- Pairs with: telemetry from v1 `trust_warn` (warn-mode) should inform whether `requireSigned` ever becomes a per-workspace default.

## Related (links)

- GH #17 (signing decision), #39 (harness v2 / HV2-4), #20 (give-feedback — the unlock)
- `modules/agent-template/interconnect/trust.md` — v1 Kalyāṇamitta-7 gate + modes
- `modules/agent-template/interconnect/routing.md` — `@workspace` seam + no-central-broker (SN 22.59)
- `modules/agent-template/interconnect/messaging.md` — envelope shape (`from`, Sāraṇīyadhamma-6)
- `crates/bwoc-cli/src/send.rs` — where `--from` + the gate live
- `notes/2026-05-23_phase3-remaining-sequencing.md` — the seam was first flagged here
