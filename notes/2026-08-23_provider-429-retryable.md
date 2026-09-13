# 2026-08-23 — Provider 429/408 made retryable, with Retry-After honoured

Closes #479. HTTP 429 (rate limit) — the single most common real-world provider
failure — was classified **fatal** and aborted the whole run, while the rarer
5xx got three backoff retries. The 200 ms → 3.2 s backoff loop was already the
right shape to absorb a 429; it just never saw one.

## What changed

- `classify_http_error` now maps **429** and **408** to `TransientProvider`
  alongside 5xx. One function fronts every backend (OpenAI-compat + Anthropic),
  so the fix is backend-neutral and covers all providers at once.
- `HarnessError::TransientProvider` became a struct variant
  `{ msg, retry_after: Option<Duration> }`. New constructors `transient()` /
  `transient_after()` keep the ~dozen call sites terse; `retry_after()` reads the
  hint back.
- The four HTTP call sites (`client.rs` ×2, `anthropic.rs` ×2) parse the
  `Retry-After` header (via `parse_retry_after`) **before** `.text()` consumes
  the response, and thread it through.
- `call_with_retry_v2` prefers the server's `Retry-After` over its computed
  backoff, via the pure `retry_delay_ms(retry_after, attempt)` helper —
  **clamped** to `RETRY_AFTER_MAX_MS` (≈ 4× `MAX_BACKOFF_MS`, ~13 s) so a
  hostile or misconfigured endpoint cannot park the run with a giant hint.

## Decisions

- **Delta-seconds only.** `parse_retry_after` honours the integer form (what
  every rate-limiter emits) and ignores the HTTP-date form — an unparseable hint
  falls back to the loop's own backoff, which is safe.
- **Struct variant over a parallel channel.** The hint has to travel *with* the
  error to reach the loop; a struct field is the honest place for it.
- Extracted `retry_delay_ms` as a pure fn so the prefer-and-clamp policy is unit-
  tested without sleeping.

## Alternatives considered

- Keeping `TransientProvider(String)` and passing the hint out-of-band — rejected;
  the error is the natural carrier and `is_transient()` already gates the loop.

## Tests

- Conformance: added a 429 case next to the 5xx assertion (both backends).
- `client.rs`: 429/408/5xx transient, other-4xx fatal, `Retry-After` threaded,
  `parse_retry_after` reads delta-seconds / ignores date+garbage+absent.
- `provider.rs`: `retry_delay_ms` prefers the hint, clamps a hostile hint, falls
  back to backoff.

## Related

- Issue #479; source `research/2026-08-23_grok-build-comparison.md`.
