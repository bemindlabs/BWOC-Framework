# 2026-08-20 — Replay defense now survives a daemon restart

Closes the last MED the L3 red-team left unresolved, chosen by the architect as
a precondition for Slice 4 rather than something to accept as a v1 risk.

## The hole

`ReplayGuard`'s seen-`(from, nonce)` set was **memory-only**, so restarting the
daemon forgot every nonce. Inside the ±300 s freshness window that is exploitable:
an attacker holding a captured, validly-signed envelope can race a restart and
replay it — the signature still verifies, because a signature proves *who*, not
*when*.

It matters more now than when the guard was written. Slice 3 made a verified
signature **mint an identity** (#452), so a replay would *re-mint* it, and Slice
4 will turn that identity into act-as-user authority. Closing it before the
switch is flipped is cheaper than reasoning about whether every act-as-user
effect is idempotent.

## Design

The freshness window does the bounding **for free**: an envelope older than the
window is refused on `ts` alone, so only the last few minutes of nonces are worth
remembering. That makes the sidecar naturally small and pruning trivial.

- `<agent_dir>/.bwoc/replay-nonces.jsonl` — one JSON line per accepted nonce,
  beside the existing `peers.toml`.
- **Load** filters by the freshness window and rewrites the file with the
  survivors (atomic tmp + rename), so the sidecar self-prunes every restart.
- **Runtime** compacts every 4096 appends, re-applying the window, so a
  long-running daemon cannot grow the file without bound.
- Written **before** `check` returns "accept": a nonce the caller acts on must
  already be durable, or a crash in between reopens the hole.
- Serialized by hand with `serde_json` — no `serde` derive dependency added to
  `bwoc-agent` for three fields (dep-quarantine).

**Best-effort on purpose.** Persistence is a hardening layered on the in-memory
guard, so an unwritable sidecar degrades to exactly the old behaviour instead of
refusing traffic — a full disk must not become a denial of service. The failure
is reported once, not silently swallowed.

## A bug I introduced and caught

The first draft's compaction wrote `Vec::new()` — it would have **silently
forgotten every nonce recorded since startup**, reopening the very hole this
change closes, and invisibly, because the in-memory guard still worked for the
rest of that run. Caught by re-reading before testing, then pinned by a test.

That test needed the compaction threshold to be reachable, so `compact_every`
became an instance field (defaulting to the const) rather than a bare constant —
a small design change bought by making the risky path testable.

## Mutation-proved

| Mutation | Result |
|---|---|
| revert to in-memory only | **red** — "a nonce recorded before the restart must still be refused" |
| compaction writes an empty file | **red** — "nonce n0 was lost across compaction + restart" |

Both were green before their tests existed — the second is precisely the bug
above, so the test earns its place.

Other cases covered: stale records pruned on load, a torn/hand-edited line
skipped rather than fatal, and an unwritable sidecar still accepting traffic
while the in-memory guard keeps catching duplicates.

## Also

`bwoc init`'s gitignore template gains `agents/*/.bwoc/replay-nonces.jsonl`
alongside the other daemon ephemerals — it is per-machine delivery state and
meaningless on another machine. Existing workspaces need the line added by hand.

## Status

59 bwoc-agent tests pass; `cargo test --workspace` clean; `clippy --workspace
--all-targets -D warnings` clean.

## Related

- Slices 1–3: `notes/2026-08-19_actas-capability-slice1.md`,
  `notes/2026-08-20_principal-forgery-clamp.md`,
  `notes/2026-08-20_verified-identity-mint.md`.
- Remaining: **Slice 4** — daemon minting, the out-of-band session actor,
  forcing `from=<self_id>`, and the peer-pin allowlist. ADR: issue #452.
