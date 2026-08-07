# 2026-08-07 — `bwoc send --all` / `--team`: broadcast fan-out

`bwoc send` gained group recipients so one message reaches a whole fleet or a
Saṅgha team in a single command — reusing the exact single-send path (signing,
route resolution, transport) per recipient.

## Why

Notifying a set of agents (e.g. "update to the new release") meant hand-sending
to each one and hunting its channel individually — the fan-out was manual and
the "who didn't get it" answer was invisible. Broadcast makes the fan-out one
command and reports per-recipient delivery.

## What changed

- **`bwoc send --all <message>`** — every agent in the workspace registry.
- **`bwoc send --team <team> <message>`** — members of `.bwoc/teams/<team>.toml`.
- Both reuse `send()` per recipient (one delivery code path). A broadcast with
  `--from <agent>` excludes that agent from its own broadcast.
- Per-recipient failures are **labeled, not fatal** (an offline gateway/MQTT
  peer relays as "not delivered live") — mirrors `bwoc ping --all`. Exit is
  non-zero only on resolution errors (no workspace / unknown team / empty set)
  or a *hard* per-recipient error.
- Output: a compact per-recipient status line (`✓ / • / ✗`, transport, verb) and
  a `N delivered, M not-live, K failed` summary.

## Decisions

- **Extend `bwoc send`, not a new `bwoc fleet broadcast`.** `send.rs` already
  owns signing, `routes.toml` resolution, MQTT/gateway relay, and the tmux
  wakeup. A second surface would duplicate all of it (Mattaññutā — one channel,
  not two).
- **Refactor `send()` to return a `SendReport` instead of printing inline.**
  Printing moved to `run` (single) and `run_group` (broadcast) so both share one
  delivery path. Chosen over adding a `quiet` field to `SendArgs`: a new field
  breaks all ~19 test literal constructions, whereas changing the `Ok` payload
  is free (tests use `.unwrap()` / `.unwrap_err()`, none destructure `Ok(())`).
- **Message disambiguation.** In broadcast mode the recipient comes from the
  flag, so clap fills the message into the `to` positional; `SendArgs::resolve`
  detects broadcast and takes the body from whichever positional is set. The
  `body` ArgGroup was relaxed to non-required; `resolve` enforces
  "message present" for both modes with an actionable error. Two inline
  positionals with a broadcast flag → ambiguous error.
- **`--reply-to` rejected for broadcast** (a fan-out has no single prior
  envelope); `kind` / `force_peer_route` / `require_signed` intentionally not
  exposed on `GroupArgs` (they belong to targeted `bwoc peer feedback`).

## Not in scope (surfaced, deferred)

The offline peers that motivated this still only get an in-memory gateway park —
broadcast reports "not delivered live" but does **not** add durable spool /
deliver-on-reconnect. That was the explicitly-deferred sibling improvement
(durable offline delivery); presence/liveness in `bwoc fleet status` is the
third. See the coordination-gap triage from this session.

## Tests

- `send.rs`: `broadcast_all_delivers_to_every_agent`, `broadcast_team_targets_only_members`,
  `broadcast_all_excludes_the_sending_agent`, `broadcast_unknown_team_is_usage_error`,
  `broadcast_empty_message_is_usage_error`.
- `main.rs` (`send_resolve_tests`): single vs group resolution, message-from-`to`
  positional, `--all`+`--team` conflict, no-message, two-positional ambiguity,
  `--reply-to` rejection, missing recipient. 850 cli tests green; fmt + clippy clean.

## Review fixes (Copilot, #417)

- **Path traversal (security).** `--team <name>` was joined into
  `.bwoc/teams/<name>.toml` unvalidated — `--team ../foo` escaped the teams dir.
  Added `is_safe_segment` (one `Normal` path component) + a refusal test.
- **Exit-code consistency.** `run_group` returned `1` for any hard error; it now
  takes the most-severe code via the shared `exit_code` map (usage-class `2`
  dominates runtime `1`), matching a single `bwoc send`.
- **Windows `\r\n`.** `read_file_body` stripped only `\n`, leaving a stray `\r`
  in the envelope; now trims both.

## Related

- `crates/bwoc-cli/src/send.rs`, `crates/bwoc-cli/src/main.rs`
- `modules/agent-template/interconnect/messaging.md` §CLI Surface
- Prior: [[2026-08-06_fleet-term]] (cross-pane wake — the single-recipient tile targeting)
