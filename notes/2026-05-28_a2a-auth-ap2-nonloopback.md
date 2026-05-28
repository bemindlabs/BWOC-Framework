# 2026-05-28 — A2A auth phase AP2: safe non-loopback bind

Second slice of the A2A auth-phase epic (#80), building on AP1 (#81). Makes a
non-loopback bind safe by **refusing to start** when no auth token is
configured, instead of the AP1 warn-and-serve. An explicit
`--allow-unauthenticated` opt-in preserves the old behaviour for trusted
networks / front-proxy setups.

## What changed

- **`bwoc-a2a` binary** — new `bind_policy(is_loopback, has_auth, allow_unauth)`
  → `Serve | Warn | Refuse`:
  - loopback **or** auth-on ⇒ `Serve` (silent; AP1 already stopped warning when
    auth is on for a non-loopback bind).
  - non-loopback + no auth + `--allow-unauthenticated` ⇒ `Warn` (serves, loud).
  - non-loopback + no auth, no flag ⇒ `Refuse` (exit `2`, remediation message).
- **`--allow-unauthenticated` flag** added to both `bwoc-a2a serve` and
  `bwoc a2a serve` (the CLI plumbs it through to the sibling binary).
- **Stale help fixed** — the `--bind` doc no longer says "a non-loopback value
  warns, since the listener has no authentication yet" (untrue since AP1); it
  now states the token / `--allow-unauthenticated` requirement.

## Decisions

- **Refuse by default, opt-in to serve open.** The issue text left it as
  "warning / is refused"; chose refuse-with-override (architect call). A token
  is cheap; an unauthenticated inbox on the network is the footgun v1 avoided by
  being loopback-only. The override exists for the legitimate "front proxy adds
  auth" / LAN-test case so the refusal is never a hard wall.
- **Exit `2`, not `1`.** Matches the existing usage-error exits in `run_serve`
  (e.g. invalid `--team`); a misconfigured bind is operator error, not a runtime
  fault.
- **Policy as a pure fn.** `bind_policy` is decision-only (no I/O), so the
  matrix is unit-tested directly without spinning a listener — the `run_serve`
  wiring just maps the three variants to stderr + exit.

## Verification

- `bind_policy_covers_the_matrix` unit test (all 7 meaningful combinations).
  `bwoc-a2a` 57 tests + `bwoc-cli` green; clippy clean.
- Live against a real `agent-erlang` listener: (1) `--bind 0.0.0.0` no token →
  refused, `EXIT=2`; (2) `+ --allow-unauthenticated` → warns + serves `auth
  OFF`; (3) `+ BWOC_A2A_TOKEN` → serves `auth ON`, no warning; (4) loopback
  default no token → serves silently (dev path unchanged).

## Status / deferred (later AP phases, #80)

- AP3 — webhook **delivery** + SSRF guard (#48-P5 deferral).
- AP4 — per-token request rate + `SubscribeToTask` concurrency caps.
- AP5 — outbound client auth (`bwoc a2a send`/`fetch-card` present credentials).

## Related

- Epic #80 (auth phase). Builds on AP1 (#81) and the v1 listener (#72).
- `crates/bwoc-a2a/src/main.rs`, `crates/bwoc-cli/src/a2a.rs`,
  `crates/bwoc-cli/src/main.rs`.
