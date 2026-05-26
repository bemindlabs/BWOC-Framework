//! ed25519 message signing — agent identity proof for inter-agent messages
//! (HV2-4 / `docs/en/SIGNING.en.md`). **Security-critical.**
//!
//! Lean by design: ed25519 + hex + canonical-JSON only, no async/HTTP, so both
//! `bwoc-cli` (sign on `bwoc send`) and `bwoc-agent` (verify in the trust gate)
//! can depend on it without pulling the harness runtime — and `bwoc-core` stays
//! crypto-free (dep-quarantine HARD RULE).
//!
//! # Keypair
//!
//! One ed25519 keypair per agent:
//! - **Private key**: hex in `<agent>/.bwoc/agent.key`, mode `0600`, gitignored.
//!   (The spec sketched the OS keyring; a 0600 file is used instead because
//!   agents run headless/CI where the keyring is unavailable — the keyring path
//!   is even `#[ignore]`d in `bwoc-harness`. The local-OS-user trust boundary
//!   makes a 0600 file an acceptable store for this threat model.)
//! - **Public key**: hex in `trust.signingPublicKey` of `config.manifest.json`,
//!   published so recipients can verify.
//!
//! # Canonical bytes (what the signature covers)
//!
//! RFC 8785 (JCS) canonical JSON over the signed fields — sorted keys, compact,
//! UTF-8. The fields are `{from, to, ts, messageId, message, nonce}`:
//! - `to` (recipient) is signed → a captured envelope can't be re-aimed.
//! - `nonce` + `ts` + `messageId` are signed → they bind the envelope to one
//!   issuance, so a time-based sliding replay window (a follow-up) can reject
//!   replays without changing the canonical form.
//!
//! Cross-language peers (#20) can reproduce the canonical form from the field
//! values with any JCS implementation — no bespoke binary layout.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};

/// Re-export so downstream crates can name the key type without depending on
/// `ed25519-dalek` directly.
pub use ed25519_dalek::SigningKey as AgentSigningKey;

/// Private-key filename inside an agent's `.bwoc/` directory.
pub const KEY_FILE: &str = "agent.key";

// ── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid hex in key material: {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("invalid ed25519 key bytes: {0}")]
    InvalidKey(#[from] ed25519_dalek::SignatureError),
    #[error("signature verification failed")]
    BadSignature,
    #[error("key file already exists at {0} (pass force to overwrite)")]
    KeyExists(PathBuf),
}

// ── Key generation + storage ─────────────────────────────────────────────────

/// Generate a fresh ed25519 keypair, store the private key at
/// `<agent_bwoc_dir>/agent.key` (hex, `0600`), and return the public key as
/// lowercase hex for the caller to publish in the manifest.
///
/// Without `force`, refuses to overwrite an existing key (`KeyExists`) so a
/// re-run never silently rotates an agent's identity.
pub fn generate_keypair(agent_bwoc_dir: &Path, force: bool) -> Result<String, SigningError> {
    let key_path = agent_bwoc_dir.join(KEY_FILE);
    if key_path.exists() && !force {
        return Err(SigningError::KeyExists(key_path));
    }
    fs::create_dir_all(agent_bwoc_dir)?;

    let signing_key = SigningKey::generate(&mut OsRng);
    let privkey_hex = hex::encode(signing_key.to_bytes());
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());

    write_key_file(&key_path, &privkey_hex)?;
    Ok(pubkey_hex)
}

/// Load an agent's signing key from `<agent_bwoc_dir>/agent.key`.
/// `Ok(None)` when the file is absent (the caller decides whether that is fatal
/// — e.g. enforce-mode send refuses an unsigned agent); `Err` when present but
/// malformed.
pub fn load_signing_key(agent_bwoc_dir: &Path) -> Result<Option<SigningKey>, SigningError> {
    let key_path = agent_bwoc_dir.join(KEY_FILE);
    if !key_path.exists() {
        return Ok(None);
    }
    let hex_str = fs::read_to_string(&key_path)?;
    let bytes = hex::decode(hex_str.trim())?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| invalid_data("expected 32-byte private key"))?;
    Ok(Some(SigningKey::from_bytes(&arr)))
}

/// Parse a verifying (public) key from the hex stored in `trust.signingPublicKey`.
pub fn load_verifying_key(pubkey_hex: &str) -> Result<VerifyingKey, SigningError> {
    let bytes = hex::decode(pubkey_hex.trim())?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| invalid_data("expected 32-byte public key"))?;
    Ok(VerifyingKey::from_bytes(&arr)?)
}

// ── Canonical bytes ───────────────────────────────────────────────────────────

/// Build the RFC 8785 (JCS) canonical bytes that are signed and verified.
///
/// A JSON object of the signed string fields, serialized with sorted keys and
/// no insignificant whitespace (`BTreeMap` guarantees the key order regardless
/// of `serde_json` features). Both signer and verifier MUST call this so the
/// bytes are identical.
pub fn canonical_bytes(
    from: &str,
    to: &str,
    ts: &str,
    message_id: &str,
    message: &str,
    nonce: &str,
) -> Vec<u8> {
    let map: BTreeMap<&str, &str> = BTreeMap::from([
        ("from", from),
        ("to", to),
        ("ts", ts),
        ("messageId", message_id),
        ("message", message),
        ("nonce", nonce),
    ]);
    serde_json::to_vec(&map).expect("BTreeMap<&str,&str> is always serializable")
}

/// A fresh 128-bit random nonce as lowercase hex (32 chars).
pub fn new_nonce() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

// ── Sign + verify ─────────────────────────────────────────────────────────────

/// Sign `payload` and return the lowercase-hex signature (128 hex chars).
pub fn sign(key: &SigningKey, payload: &[u8]) -> String {
    hex::encode(key.sign(payload).to_bytes())
}

/// Verify a hex `sig` over `payload` against `verifying_key`.
/// `Ok(())` on success; `Err(BadSignature)` on any failure (bad hex, wrong
/// length, or cryptographic mismatch) — all failure modes collapse to one
/// non-informative error so a verifier can't be probed.
pub fn verify(
    verifying_key: &VerifyingKey,
    payload: &[u8],
    sig_hex: &str,
) -> Result<(), SigningError> {
    let sig_bytes = hex::decode(sig_hex.trim()).map_err(|_| SigningError::BadSignature)?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| SigningError::BadSignature)?;
    let sig = Signature::from_bytes(&sig_arr);
    verifying_key
        .verify(payload, &sig)
        .map_err(|_| SigningError::BadSignature)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn invalid_data(msg: &'static str) -> SigningError {
    SigningError::Io(io::Error::new(io::ErrorKind::InvalidData, msg))
}

/// Write `hex_key` to `path` and restrict it to `0600` on Unix. On non-Unix the
/// file is still written; the operator must secure it.
fn write_key_file(path: &Path, hex_key: &str) -> Result<(), io::Error> {
    fs::write(path, format!("{hex_key}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_canonical(message: &str, nonce: &str) -> Vec<u8> {
        canonical_bytes(
            "agent-alpha",
            "agent-beta",
            "2026-05-26T10:00:00Z",
            "msg-20260526T100000Z-ab123",
            message,
            nonce,
        )
    }

    #[test]
    fn keygen_creates_0600_key_file_and_returns_pubkey_hex() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey = generate_keypair(&bwoc, false).unwrap();
        assert_eq!(pubkey.len(), 64, "pubkey hex: {pubkey}");
        assert!(pubkey.chars().all(|c| c.is_ascii_hexdigit()));

        let key_path = bwoc.join(KEY_FILE);
        assert!(key_path.exists());
        assert_eq!(fs::read_to_string(&key_path).unwrap().trim().len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&key_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "private key must be 0600");
        }
    }

    #[test]
    fn keygen_refuses_overwrite_without_force() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        generate_keypair(&bwoc, false).unwrap();
        assert!(matches!(
            generate_keypair(&bwoc, false).unwrap_err(),
            SigningError::KeyExists(_)
        ));
        // force overwrites.
        assert_eq!(generate_keypair(&bwoc, true).unwrap().len(), 64);
    }

    #[test]
    fn sign_verify_roundtrip() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey = generate_keypair(&bwoc, false).unwrap();
        let key = load_signing_key(&bwoc).unwrap().unwrap();

        let payload = sample_canonical("hello", "00112233445566778899aabbccddeeff");
        let sig = sign(&key, &payload);
        assert_eq!(sig.len(), 128, "64-byte sig = 128 hex chars");

        let vk = load_verifying_key(&pubkey).unwrap();
        verify(&vk, &payload, &sig).expect("valid signature must verify");
    }

    #[test]
    fn tampered_field_fails_verify() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey = generate_keypair(&bwoc, false).unwrap();
        let key = load_signing_key(&bwoc).unwrap().unwrap();

        let sig = sign(&key, &sample_canonical("original", "0000"));
        let vk = load_verifying_key(&pubkey).unwrap();
        // Any change to a signed field invalidates the signature.
        let result = verify(&vk, &sample_canonical("tampered", "0000"), &sig);
        assert!(matches!(result, Err(SigningError::BadSignature)));
    }

    #[test]
    fn wrong_pubkey_fails_verify() {
        let d1 = tempdir().unwrap();
        let d2 = tempdir().unwrap();
        let b1 = d1.path().join(".bwoc");
        let b2 = d2.path().join(".bwoc");
        generate_keypair(&b1, false).unwrap();
        let pubkey2 = generate_keypair(&b2, false).unwrap();

        let key1 = load_signing_key(&b1).unwrap().unwrap();
        let payload = sample_canonical("hi", "abcd");
        let sig = sign(&key1, &payload);

        let vk2 = load_verifying_key(&pubkey2).unwrap();
        assert!(matches!(
            verify(&vk2, &payload, &sig),
            Err(SigningError::BadSignature)
        ));
    }

    #[test]
    fn garbage_sig_hex_fails_cleanly() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey = generate_keypair(&bwoc, false).unwrap();
        let vk = load_verifying_key(&pubkey).unwrap();
        // Not hex / wrong length must be BadSignature, never a panic.
        assert!(matches!(
            verify(&vk, &sample_canonical("x", "y"), "not-hex-zzz"),
            Err(SigningError::BadSignature)
        ));
        assert!(matches!(
            verify(&vk, &sample_canonical("x", "y"), "dead"),
            Err(SigningError::BadSignature)
        ));
    }

    #[test]
    fn load_signing_key_absent_is_none() {
        let dir = tempdir().unwrap();
        assert!(
            load_signing_key(&dir.path().join(".bwoc"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn canonical_is_sorted_compact_json() {
        let bytes = canonical_bytes("a", "b", "t", "m", "msg", "n");
        let s = String::from_utf8(bytes).unwrap();
        // Keys sorted (from, message, messageId, nonce, to, ts), no whitespace.
        assert_eq!(
            s,
            r#"{"from":"a","message":"msg","messageId":"m","nonce":"n","to":"b","ts":"t"}"#
        );
    }

    #[test]
    fn canonical_is_deterministic_regardless_of_arg_order_effect() {
        // Same field values → identical bytes every call (no map iteration order
        // leakage). Guards the signer/verifier agreement invariant.
        let a = canonical_bytes("x", "y", "ts", "id", "body", "nn");
        let b = canonical_bytes("x", "y", "ts", "id", "body", "nn");
        assert_eq!(a, b);
    }

    #[test]
    fn new_nonce_is_32_hex_and_unique() {
        let a = new_nonce();
        let b = new_nonce();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "nonces must differ (2^-128 collision)");
    }
}
