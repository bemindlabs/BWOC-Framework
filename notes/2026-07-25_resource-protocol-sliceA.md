# 2026-07-25 — Resource Protocol (fleet compute & memory sharing), slice A

A new protocol so a light host (a laptop agent) can borrow compute (GPU/CPU),
working memory (RAM, shared KV/context), or read-only federated knowledge from a
heavy host (a GPU server) across the fleet — brokered through the `bwoc-gateway`
relay under a refuse-by-default provider sharing gate. Requested as "protocol to
use compute power and memory from other fleet servers", scope confirmed as all
four dimensions, via the gateway, spec + implement.

## What changed

- **Spec** `docs/{en,th}/RESOURCE-PROTOCOL` — the full design: actors
  (provider / consumer / broker), three resource kinds (`compute` / `kv` /
  `knowledge`), the lease lifecycle (advertise → discover → claim → lease → use
  → release), the resource snapshot, the sharing gate + caps, the signed-envelope
  wire format over the gateway, the security model, philosophy grounding, and the
  A/B/C slicing.
- **Code** `crates/bwoc-cli/src/resource.rs` + `main.rs` wiring — slice A:
  - `bwoc resource snapshot [--json]` — GPU (`nvidia-smi`), CPU cores
    (`available_parallelism`), RAM + load (`/proc/meminfo`, `/proc/loadavg`).
  - `bwoc resource gate-check --kind K --from A [--gpu-vram N] [--ram N] [--cores N]`
    — dry-runs the sharing gate against this host's `[resource]` config + a live
    snapshot; typed allow/deny.
  - Shared types (`ResourceSnapshot`, `Gpu`, `SharingConfig`/`Caps`, `ClaimSpec`,
    `DenyReason`, `ResourceKind`) + `evaluate_gate` (pure) + `load_sharing_config`.
- 13 unit tests: the three pure parsers + the five-step gate (each denial reason,
  over_cap-vs-insufficient_free distinction, empty-allow-permits-any, grant).

## Decisions

- **Slice A = local + no-network only.** `snapshot` and `gate-check` need nothing
  but the host itself, so they ship first and prove the types/gate end-to-end
  without waiting on the broker. `advertise`/`discover`/`claim` are specified but
  deferred — they have no real counterpart to talk to until the gateway registry
  (slice B) exists. Building them now would mean stubbed transport + dead code
  (Mattaññutā — don't add what has no caller yet).
- **`gate-check` as the slice-A gate exerciser.** Rather than ship an `evaluate_gate`
  with only test callers (dead code in a bin crate), `gate-check` gives an operator
  a genuinely useful diagnostic ("will my caps allow this?") that exercises config
  parse + snapshot + the full gate.
- **Refuse-by-default sharing gate, IAM-grade.** A host shares nothing until
  `[resource] share = true`; every claim is gated by kind/allow/caps/free-fit. Same
  shape as the financial-write / IAM gates, grounded in Fleet Governance §6
  (*cetiya* — honor shared resources).
- **`compute` folds RAM in.** "Borrow RAM for a job" is a constraint on a compute
  claim, not a separate kind — one lease lifecycle, three kinds (`compute`/`kv`/
  `knowledge`), not four.
- **Dumb untrusted broker.** The gateway matches offers and relays signed envelopes;
  it executes nothing and sees no plaintext secrets. Trust lives in ed25519
  signatures (reuse `cc-signing`), same principle as the existing relay.
- **`max_leases` + `gateway` parsed but not yet read.** Concurrent-lease counting is
  broker-side state (slice B) and can't be evaluated from a single snapshot; the
  fields are parsed now (so a config author isn't rejected) and `#[allow(dead_code)]`
  until their slice.

## Alternatives considered

- **Four resource kinds (separate `ram`).** Rejected — RAM is a compute-claim
  constraint; a separate kind duplicates the lifecycle for no gain.
- **Build advertise/claim now against a stub broker.** Rejected — stubbed transport
  is dead weight; the gateway (slice B) gives them a real endpoint.
- **Add `sysinfo`/`num_cpus` deps for cross-platform RAM.** Rejected for slice A —
  providers are Linux GPU servers; `/proc` + `available_parallelism` (std) cover
  them. macOS RAM shows "unavailable" (a consumer host doesn't advertise). Revisit
  if a non-Linux provider ever matters.

## Status / deferred

- Shipped 2.39.0 (`v2026.7.25-1`), slice A.
- **Slice B (bwoc-gateway):** `/v1/resource/*` routes, TTL-evicted offer registry,
  discover matching, claim forwarding.
- **Slice C (framework):** `advertise` heartbeat, `discover`/`claim`/`release`
  clients, `compute` offload execution (claim → run on provider via A2A → release),
  then `kv` + `knowledge`. Per-claim `evaluate_gate` full path + `max_leases`.

## Related

- `docs/en/RESOURCE-PROTOCOL.en.md`, `crates/bwoc-cli/src/resource.rs`
- `docs/en/FLEET-GOVERNANCE.en.md` §6 (the charter), `docs/en/SIGNING.en.md` (the wire)
- gateway repo: `bwoc-gateway` (becomes the broker)
