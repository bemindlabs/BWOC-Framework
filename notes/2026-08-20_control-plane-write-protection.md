# 2026-08-20 — An untrusted turn may not write its own control plane

Found while red-teaming the **design** for #452 Slice 4 — but this is not a
Slice-4 issue. It is live today, and it is why Slice 4 is **not** being built.

## The hole

The Layer-0 capability gate grades `write_file` / `edit_file` as
`Capability::WorktreeWrite`, allowed on an untrusted turn "when the target
resolves inside the worktree". The check was **only** `confine_path`.

But `<worktree>/.bwoc/` *is inside the worktree*. So a confined write reached
every file that decides what the agent is allowed to do:

| File | Consequence of writing it |
|---|---|
| `.bwoc/harness-policy.toml` | **the permission policy itself** — rewrite to allow-all and Layer 2 stops objecting |
| `.bwoc/peers.toml` | pinned signing keys — plant one and you can mint a "verified" peer |
| `.bwoc/replay-nonces.jsonl` | replay defense — truncate it and captured envelopes replay |
| `.bwoc/inbox.refusals.jsonl` | the refusal audit trail — rewrite it and the record of what was blocked is gone |
| `.bwoc/workspace.toml`, `agents.toml`, `interconnect/` | registry + routing: who exists, who is reachable |
| `config.manifest.json` | the agent's own backend, model and trust posture |

Verified by reading: no `.bwoc` protection exists anywhere in `guardrails.rs` or
`sandbox.rs`, and the `WorktreeWrite` arm called `confine_path` and nothing else.

In the shipped configurations Layer 2 happened to stop it (the chat policy asks,
and auto-process auto-denies). That is luck of configuration, not a gate — and
Slice 4's design proposed punching a hole in exactly that layer, which is how the
red-team surfaced it.

## Fix

`is_control_plane(resolved, worktree_root)` — checked on the **resolved** path,
so `memories/../.bwoc/peers.toml`, `./.bwoc/x` and symlink games are all caught
by one rule rather than a blocklist of spellings.

Scope: any `.bwoc` component under the worktree, plus `config.manifest.json`.
Nothing legitimate is denied — the harness writes its own `.bwoc/chat-session.json`
through `std::fs` directly, never through a tool, and no tool has business
writing there. Trusted turns are unaffected; Layer 0 only ever gates untrusted
ones.

## A bug the test caught immediately

The first draft compared against the **raw** `worktree_root`, but `confine_path`
resolves against a *canonicalized* root — so on macOS (`/var` → `/private/var`)
`strip_prefix` silently failed and the rule was a **no-op on the very platform it
was being tested on**. The test went red on the first run and named the file.

Now: strip against the canonicalized root, and on any unexpected prefix mismatch
**fail closed** (treat any `.bwoc` component as control plane) rather than
silently under-blocking.

## Mutation-proved

| Mutation | Result |
|---|---|
| remove the control-plane check (the shipped state) | **red** — "`.bwoc/harness-policy.toml` … must be denied … got Proceed" |
| compare against the raw, non-canonicalized root | **red** |

Plus two guard tests so the rule cannot over-reach: ordinary content writes
(`src/main.rs`, `memories/recall.md`, and the near-miss `a.bwoc.txt`) still
proceed, and a **trusted** turn is unaffected.

## Why Slice 4 is not being built

The red-team returned `holds=false` on all three lenses (25 attacks). Its central
finding: the design's two conjuncts — "crypto verified" **and** "operator pinned"
— are **not independent**, because `.bwoc/peers.toml` supplies both and sat in
the agent's own writable worktree. A second confirmed break: the resolver tries
`resolve_peer_manifest` (routes.toml, peer-published key) *before*
`resolve_pinned_peer`, so for a peer that is both routed and pinned the signature
verifies against a key the operator never pinned — the pin is decorative. And an
unresolved HIGH: act-as-user cannot even execute in the standalone/gateway
deployment `peers.toml` exists for, since `bwoc send` hard-requires a local
workspace.

Per the ticket's own rule — *build the seam with its first consumer* — and with
the inbound-service consumer still absent, enabling act-as-user now would add
attack surface nobody uses. This PR takes the one finding that is a genuine
present-day hardening and leaves the switch off.

## Status

512 harness tests pass; `clippy --all-targets --features test-redteam -D warnings`
clean.

## Related

- Design + red-team for Slice 4 (this PR's origin). ADR: issue #452.
- Still open from the red-team, **not** fixed here: the resolver-order finding
  (routed key beats pinned key).
