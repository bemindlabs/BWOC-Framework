# 2026-05-27 — HV2-4 message signing: implementation

Built the ed25519 message-signing vertical slice (HV2-4 / BWOC-3) after the §9
spec gate was ratified. Identity is now cryptographically provable end-to-end:
keygen → publish pubkey → sign on `bwoc send` → verify in the `bwoc-agent`
trust gate, enforce-by-default. Validated on the 9 live agents in the parent
workspace. The earlier PR #40 (closed) was the reference for the crypto core.

## What changed

- **New crate `bwoc-signing`** (lean: `ed25519-dalek` + `rand_core` + `hex` +
  `serde_json`; no async/HTTP). Keygen (file-backed), JCS canonical bytes,
  sign/verify, nonce. 10 unit tests.
- **`bwoc-core` manifest** — added `trust.signingPublicKey` (hex public key).
- **`bwoc send`** — signs the envelope (adds `nonce` + `sig`) when the sender
  is an agent with a key; warns and sends unsigned otherwise.
- **`bwoc trust --keygen [agent|--all] [--force]`** — generates keypair(s),
  writes the private key to `<agent>/.bwoc/agent.key` (0600) and the public key
  to the manifest. `--all` backfills existing agents.
- **`bwoc-agent` trust gate** — `evaluate` now runs a signature step before the
  Kalyāṇamitta quality gate (§5: verify, then authorize). New `SigningMode`
  (`off|warn|enforce`, default **Enforce**, via `BWOC_SIGNING_MODE`).
- `bwoc-cli` and `bwoc-agent` now depend on `bwoc-signing`.

## Decisions (autonomous — flagged for review)

- **Crypto lives in a new `bwoc-signing` crate, not `bwoc-core` or `bwoc-harness`.**
  The spec assumed `bwoc-harness`-only, but the sign point (`bwoc send`) and
  verify point (`bwoc-agent`) are both in crates that do **not** depend on the
  harness, and the dep-quarantine HARD RULE forbids crypto in `bwoc-core`. A
  lean shared crate satisfies all three. *(ratified in session)*
- **Private key in a 0600 file (`.bwoc/agent.key`), not the OS keyring** — spec
  §3 said keyring, but agents run headless/CI where the keyring is unavailable
  (the harness keyring test is `#[ignore]`d for the same reason). PR #40 made
  the same call. The local-OS-user trust boundary makes a 0600 file acceptable.
  **Spec §3 updated to match.**
- **Canonical form = RFC 8785 JCS** (§9.4 ratified) over `{from,to,ts,
  messageId,message,nonce}`, via a sorted `BTreeMap` → `serde_json`. Replaces
  PR #40's pipe-concat; language-agnostic for cross-workspace peers (#20).
- **Enforce by default** (§9.6, operator override). A bad/tampered signature is
  refused in *every* mode (it is an attack); only unsigned / unpublished-key /
  malformed-key cases are downgraded to proceed under `warn`. Signature
  verification is independent of the `BWOC_TRUST_GATING` Kalyāṇamitta opt-in.
- **`nonce` is signed but the sliding replay window is deferred** (a follow-up,
  as PR #40 also scoped). Signing + recipient-binding close the primary forgery
  threat; including `nonce`/`ts`/`messageId` in the canonical form lets the
  window land later without changing the wire/canonical format.
- **`--keygen` is a flag on `bwoc trust`, not a `bwoc trust keygen` subcommand**,
  to preserve the existing `bwoc trust <agent>` read surface. (Minor UX wart:
  `bwoc trust keygen` parses `keygen` as the agent positional.)

## Validation

- `cargo test --workspace` green; `clippy` clean on the changed crates.
- `bwoc trust --keygen --all` backfilled all 9 agents (keys + published pubkeys).
- `bwoc send --from agent-yudi agent-zhongkui` produced a `nonce`+`sig` envelope;
  canonical call sites match field-for-field between sign and verify (covered by
  `bwoc-agent::valid_signature_proceeds_*` and the crate roundtrip test).

## Status / deferred

- Vertical slice complete + validated. **Not yet**: time-based sliding replay
  window; `bwoc new` keygen-at-incarnation (only backfill exists); verification
  in the interactive `bwoc inbox` path (today only the `bwoc-agent` daemon
  verifies — same as the Kalyāṇamitta gate); `.gitignore` of `agent.key` for
  user workspaces created by `bwoc init`.
- **Docs:** SIGNING.en/th §3 updated to file storage; a fuller §4/§9
  "ratified ✓" pass is a doc follow-up.

## Related

- [`docs/en/SIGNING.en.md`](../docs/en/SIGNING.en.md) (renamed from TRUST-V2).
- Closed PR #40 (reference), #41/#42 (superseded by #57). GH #39, #20.
- [`2026-05-26_harness-v2-review-fixes.md`](2026-05-26_harness-v2-review-fixes.md).
