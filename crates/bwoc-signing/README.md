# bwoc-signing

ed25519 message-signing primitives for [BWOC](../../README.md) agent identity proof.

Both the sign side ([`bwoc-cli`](../bwoc-cli/), on `bwoc send`) and the verify side ([`bwoc-agent`](../bwoc-agent/), in the trust gate) depend on this crate. It is deliberately lean — ed25519, hex, canonical JSON, `thiserror`; no async, no HTTP — so neither side pulls the harness runtime to sign or verify, and [`bwoc-core`](../bwoc-core/) stays crypto-free under the dep-quarantine rule. Single flat module, no submodules. Protocol details: [`SIGNING.en.md`](../../docs/en/SIGNING.en.md).

## Scope

- **`generate_keypair(dir, force)`** — fresh keypair; writes the private key as hex to `<dir>/agent.key` at mode `0600` on Unix, returns the public key hex for `trust.signingPublicKey` in `config.manifest.json`. Without `force` it refuses to overwrite (`SigningError::KeyExists`) so a re-run never silently rotates an identity.
- **`load_signing_key(dir)`** — reads `<dir>/agent.key`; `Ok(None)` when absent (the caller decides whether that is fatal), `Err` when present but malformed.
- **`load_verifying_key(hex)`** — parses the published public-key hex into a `VerifyingKey`.
- **`canonical_bytes(from, to, ts, message_id, message, nonce)`** — RFC 8785 (JCS) canonical JSON over the signed fields: sorted keys via `BTreeMap`, compact, UTF-8. Signer and verifier must both call it so the bytes are byte-identical. Signing `to`, `nonce`, `ts`, and `messageId` means a captured envelope can't be re-aimed or replayed.
- **`new_nonce()`** — 128-bit OS-random nonce, lowercase hex (32 chars).
- **`sign(key, payload)` / `verify(vk, payload, sig_hex)`** — hex signature (128 chars) in, `Ok(())` or `SigningError::BadSignature` out. Every verify failure mode — bad hex, wrong length, cryptographic mismatch — collapses to that one non-informative error so a verifier can't be probed.
- **`KEY_FILE`**, **`AgentSigningKey`** (re-export of `ed25519_dalek::SigningKey`), **`SigningError`**.

## Usage

In another crate within the workspace:

```toml
[dependencies]
bwoc-signing = { workspace = true }
```

```rust
use std::path::Path;

let bwoc_dir = Path::new("agents/agent-alpha/.bwoc");
let pubkey_hex = bwoc_signing::generate_keypair(bwoc_dir, false)?;

let key = bwoc_signing::load_signing_key(bwoc_dir)?.expect("key just written");
let nonce = bwoc_signing::new_nonce();
let payload = bwoc_signing::canonical_bytes(
    "agent-alpha",
    "agent-beta",
    "2026-05-26T10:00:00Z",
    "msg-20260526T100000Z-ab123",
    "hello",
    &nonce,
);
let sig = bwoc_signing::sign(&key, &payload);

let vk = bwoc_signing::load_verifying_key(&pubkey_hex)?;
bwoc_signing::verify(&vk, &payload, &sig)?;
```

Keypairs are generated from the CLI with `bwoc trust --keygen <agent>` (or `--keygen --all` to backfill, `--force` to rotate).

## Status

Complete and in use — keygen, canonical bytes, sign, and verify are wired into `bwoc send` and the `bwoc-agent` trust gate, and covered by unit tests for the sign/verify roundtrip, tampered fields, wrong public key, malformed signature hex, and canonical-form stability.

## License

[MIT](../../LICENSE).
