# 2026-05-28 — A2A auth phase AP4: rate limit + subscription cap

Fourth slice of the A2A auth-phase epic (#80), after AP1 (#81, inbound auth),
AP2 (#82, safe bind), and AP3 (#83, webhook delivery). Adds the two resource
guards #48 deferred until there was an authenticated, exposed endpoint to
attribute load to: a request **rate limit** and a `SubscribeToTask`
**concurrency cap**.

## What changed

- **`RateLimiter`** — a global token bucket (`RATE_CAPACITY = 120` burst,
  `RATE_REFILL_PER_SEC = 60`). Refill is computed lazily from elapsed time (no
  background timer). Checked in `json_rpc` right after the auth gate; an empty
  bucket answers `429 Too Many Requests` + `Retry-After: 1`.
- **`SubGuard` + `MAX_SUBSCRIPTIONS = 32`** — an atomic active-stream counter in
  `ServeState`. `subscribe_task` reserves a slot before any work; the guard is
  moved into the SSE stream so the slot is released on **any** end (client
  disconnect, terminal state, the lifetime cap, or an early pre-flight error —
  all drop the guard). Over the cap ⇒ a `RESOURCE_EXHAUSTED` (`-32000`) JSON-RPC
  error, consistent with the other `SubscribeToTask` pre-flight errors.

## Decisions

- **Global bucket, not per-IP.** AP1 uses a single shared bearer token, so
  "per-token" degenerates to one bucket; global is faithful to that, simplest,
  and has no unbounded per-client map to grow/evict. Per-IP fairness can be
  revisited if/when multi-token or untrusted multi-tenant exposure lands.
- **Guards apply always, not only when auth is on.** `--allow-unauthenticated`
  (AP2) can still expose the listener, so the guards are unconditional resource
  protection — like the existing body-size and `SUBSCRIBE_MAX` caps. (Under
  auth-off-loopback dev they're effectively free.)
- **Fixed constants, no flags.** Matches `MAX_REQUEST_BYTES` / `SUBSCRIBE_MAX`;
  Mattaññutā — no new CLI surface until an operator actually needs to tune.
- **Rate check after auth.** An unauthenticated flood is already `401`'d, so it
  can't spend the budget and lock out a valid peer.
- **`SubGuard` claims optimistically (`fetch_add` then yield back on overflow)**
  so the check-and-increment is atomic without a lock.

## Verification

- `bwoc-a2a` 66 lib + 6 bin tests; full workspace + clippy green.
  - `RateLimiter::try_acquire_at` with an injected clock: spend, refill-per-sec,
    capacity clamp after long idle, empty-bucket.
  - `429` path: drain the bucket then a real request through `app()` is throttled.
  - Concurrency cap: fill all 32 slots (held open), the 33rd gets
    `RESOURCE_EXHAUSTED`, then dropping a held stream re-admits a new one
    (proves the `Drop` release).
- Live against a real `bwoc a2a serve`: 300 requests at 64-way parallelism →
  141 × `200`, 159 × `429`, with `retry-after: 1` on the throttled ones (≈ the
  120 burst + ~1s refill let through).

## Status / deferred (last AP phase, #80)

- AP5 — outbound client auth (`bwoc a2a send`/`fetch-card` present credentials
  to a remote agent, honoring its declared scheme). Closes the epic.
- Possible later: per-IP rate fairness, operator-tunable limits via flags.

## Related

- Epic #80. Builds on AP1 (#81), AP2 (#82), AP3 (#83); realizes the rate/
  concurrency guards deferred in the P1/P3 `serve.rs` comments (#48).
- `crates/bwoc-a2a/src/serve.rs`.
