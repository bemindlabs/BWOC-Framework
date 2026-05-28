# 2026-05-28 — A2A auth phase AP3: webhook delivery + SSRF guard

Third slice of the A2A auth-phase epic (#80), building on AP1 (#81, inbound
auth) and AP2 (#82, safe bind). Ships the push-notification **delivery** that
P5 (#48) deliberately deferred: when auth is on, the listener watches the team
task list and POSTs `TaskStatusUpdateEvent`s to registered webhooks, behind an
SSRF egress guard.

## What changed

- **`ssrf.rs` (new)** — egress guard. `blocked_reason(IpAddr)` classifies the
  ranges an SSRF pivots through (loopback, RFC 1918 private, CGNAT, link-local
  incl. the `169.254.169.254` cloud-metadata endpoint, broadcast / unspecified /
  multicast / documentation, IPv6 ULA `fc00::/7` + link-local `fe80::/10`, and
  IPv4-mapped — classified by the embedded v4). `validate(url, allow_loopback)`
  requires https (http only for loopback, a test affordance), resolves the
  host, rejects if **any** address is blocked, and returns the validated
  addresses for connection pinning.
- **`client::deliver_push`** — POSTs the event after `ssrf::validate`, with the
  connection **pinned** to the validated address(es) via
  `reqwest::resolve_to_addrs` (closes the DNS-rebind window between check and
  connect). Presents the config's token as `Authorization: Bearer`. Best-effort:
  non-2xx → `ClientError::Status`, tighter 10s timeout.
- **`serve::push_delivery_loop`** — a tokio task spawned by `run()` **only when
  auth is on AND a `--team` is set**. Polls `tasks.jsonl` (off-executor via
  `spawn_blocking`), diffs per-task state with `collect_changes`, and delivers
  to each matching push config. The first read seeds the baseline silently — the
  watcher fires only on transitions it observes, not a burst of current states.
- **`push::status_event`** — the bare `TaskStatusUpdateEvent` shape, now the one
  source for both the webhook body and the SSE `result` (serve's `status_update`
  delegates to it).

## Decisions

- **Deliver only when auth is on.** `push.rs` deferred delivery because, under
  no-auth, a client could register an external sink for another task's updates
  (exfil). Gating delivery on a configured token means the registrant was an
  authenticated peer — directly the condition that deferral named. Auth-off ⇒
  configs are still stored, just inert (status quo).
- **Pin to a validated IP (vs. pre-resolve + reconnect).** A resolve-then-check
  that lets reqwest re-resolve on connect leaves a TOCTOU rebind window; pinning
  guarantees check-IP == connect-IP. A few extra lines for the correct SSRF
  posture in a security framework.
- **No new dependency.** `reqwest::Url` parses, std `IpAddr` classifies (with
  hand-rolled CGNAT / IPv6 ULA / link-local checks where std helpers are
  unstable), `tokio::net::lookup_host` resolves — nothing added to `Cargo.toml`.
- **Best-effort, no retry/queue (yet).** First cut logs failures and moves on; a
  durable retry/backoff queue is deferred (would be its own slice if needed).
- **`allow_loopback` is test-only.** Production always passes `false`; the flag
  lets the wiremock egress test target a local mock past the loopback block.

## Verification

- `bwoc-a2a` 63 lib + 6 bin tests; full workspace + clippy green.
  - SSRF: full range matrix (v4/v6, CGNAT, metadata, IPv4-mapped), scheme rules,
    loopback affordance, garbage URL.
  - Delivery (`deliver_push`) over a real **wiremock** server: asserts the POST
    body shape, `Authorization: Bearer`, IP pinning, SSRF refusal of a private
    target, and non-2xx mapping.
  - Watcher logic (`collect_changes`): silent seed, only-transitions, terminal
    flag on `Completed`, per-task isolation.
- A live `bwoc a2a serve`-driven delivery was **not** run: the production guard
  intentionally blocks the loopback targets a local test would use, so the
  end-to-end POST is exercised against the wiremock server instead.

## Status / deferred (later AP phases, #80)

- AP4 — per-token request rate limit + `SubscribeToTask` concurrency cap.
- AP5 — outbound client auth (`bwoc a2a send`/`fetch-card` present credentials).
- Delivery retry/backoff + a per-webhook failure budget, if real use needs it.

## Related

- Epic #80. Builds on AP1 (#81), AP2 (#82), the P5 push-config store (#77),
  and the P4 outbound client (#76).
- `crates/bwoc-a2a/src/ssrf.rs`, `client.rs`, `serve.rs`, `push.rs`, `lib.rs`.
