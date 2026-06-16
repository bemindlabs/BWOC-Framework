# 2026-06-16 — bwoc-connect: env-first token + connector observability (issue #305)

On macOS the `bwoc-connect telegram` bridge started but **never called
`getUpdates`** — `pending_update_count` stayed fixed, no harness spawned, no
reply, and crucially **no stderr/log even with `RUST_LOG=debug`**. Direct
`curl …/getUpdates` worked; the bridge's poll did not.

## Root cause (most likely) + why it was invisible

`resolve_token` checked the **OS keyring first**, env second. A Keychain read
from a daemon-spawned subprocess (the connector child of `bwoc-agent --serve`)
has no interactive session to answer an authorization prompt, so
`keyring::Entry::get_password()` can **block indefinitely** — wedging the process
*before* `getMe`/the poll loop. That fits "stays alive, does no polling". And it
explains why "`TELEGRAM_BOT_TOKEN` env also reproduces": env was only consulted
*after* the keyring, so a set env var never got a chance.

It was invisible because (a) the connector's only logs are `eprintln!` (so
`RUST_LOG` does nothing) and the poll loop logged nothing on the success path,
and (b) `bwoc-agent --serve` spawned the child with **inherited stdio**, which
goes to the void under launchd.

## What changed

- **Token resolution is env-FIRST** (`bwoc-connect/main.rs`) — env is explicit
  and can never block, so the documented `TELEGRAM_BOT_TOKEN` path no longer
  touches the keychain. Keyring is the fallback.
- **Keyring read is timeout-bounded** — runs on a worker thread with a 5 s
  `recv_timeout`; a hung Keychain prompt degrades to `None` (→ env error) instead
  of wedging the connector forever.
- **Poll-loop observability** (`bwoc-connect/lib.rs::run_bridge`) — logs
  "poll loop started", "polling active (first getUpdates ok)", and a per-batch
  "drained N message(s)". "Is it polling?" is now answerable.
- **Connector stdio captured** (`bwoc-agent/connectors.rs`) — the child's
  stdout+stderr go to `<agent>/.bwoc/connector.log` instead of being inherited
  into the void, so a failing connector is now readable.

## Honesty / verification

Could not reproduce live (would hit real Telegram + reply to real users). The
env-first reorder is the most likely fix and is well-motivated by the evidence;
the bounded keyring + connector.log + poll logs guarantee that **if** something
else is wrong, the next run leaves a readable error in `.bwoc/connector.log`
rather than failing silently. No env-mutation unit test (edition-2024 `set_var`
is `unsafe` + races parallel tests — the codebase avoids these by convention).

## Related

- issue #305; `crates/bwoc-connect/src/{main.rs,lib.rs}`, `crates/bwoc-agent/src/connectors.rs`
