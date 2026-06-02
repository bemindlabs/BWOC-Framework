# 2026-06-02 — gitignore the Trust v2 agent signing private key

A workspace audit found that `bwoc init`'s `.gitignore` template never excluded
`agents/*/.bwoc/agent.key` — the ed25519 **private** key that `bwoc trust
--keygen` writes. Since user workspaces *track* `agents/` (only daemon
ephemerals were ignored), a `git add -A` after keygen would commit an agent's
private identity key. Added the pattern to the shared template tail.

## What changed

- `crates/bwoc-cli/src/init.rs` — `GITIGNORE_REST` gains an `agents/*/.bwoc/agent.key`
  block (with prose: private key 0600 on Unix, public key tracked in manifest). Placed in
  the shared tail so it applies to both the default and `--no-runtime` heads.
- Two unit tests (`init_default_runtime_and_fleet`, `init_no_runtime_omits_daemon_gitignore`)
  gained an assertion that the key pattern is present. The exact-match assertion in
  the default test still holds since it references `GITIGNORE_REST` directly.
- `CHANGELOG.md` — `[Unreleased] → Security` entry.

## Decisions

- **Tail, not daemon block.** The private key is a secret regardless of whether the
  workspace ever spawns a daemon, so it belongs next to `.bwoc/secrets/` in the
  shared tail — not in the daemon-ephemeral head that `--no-runtime` strips.
- **Public key stays tracked.** `trust.signingPublicKey` in the manifest is meant to
  be published so recipients can verify; only the private `agent.key` is ignored.

## Context

- Trust v2 signed envelopes (the `bwoc-signing` crate, ed25519 + JCS canonical bytes
  + nonce/ts replay-binding) is already implemented and wired into `bwoc send`
  (sign) and `bwoc-agent` (verify in the trust gate, Warn/Enforce modes). This was
  the original intent of the closed PR #40 (which inlined crypto into `bwoc-core`);
  it shipped instead via the dedicated dep-quarantined `bwoc-signing` crate. The
  gitignore line was the one piece of #40's diff that did not carry over.
- `docs/en/ROADMAP.en.md` still lists Trust v2 as "deferred, off the DoD" — stale.
  A separate doc-sync change should move it to shipped (not bundled here — one
  concern per PR).

## Status / deferred

- Done: gitignore fix + tests + CHANGELOG. fmt + clippy + the two init tests pass locally.
- Deferred (separate PR): ROADMAP/VERSION doc-sync for Trust v2 shipped status.
