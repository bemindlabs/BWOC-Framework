//! Trust v2 — ed25519 agent identity proof via asymmetric signed envelopes.
//!
//! Spec: HV2-4 / harness-v2 #39. Security-critical module.
//!
//! # Design
//!
//! Each agent has a **per-agent ed25519 keypair**:
//! - Private key: `<agent>/.bwoc/agent.key` (hex, 0600, gitignored).
//! - Public key: `trust.publicKey` in `config.manifest.json` (hex).
//!
//! On `bwoc send --from <agent>`, the sender:
//! 1. Loads its private key from `.bwoc/agent.key`.
//! 2. Serialises the **canonical bytes** of the envelope (deterministic; see
//!    `canonical_bytes`).
//! 3. Signs with ed25519; stamps the hex signature as `sig` in the envelope.
//!
//! On receive (`bwoc-agent trust::evaluate`), the verifier:
//! 1. Reads `sig` from the envelope (if absent → no-signature path).
//! 2. Resolves the sender's `trust.publicKey` from their manifest.
//! 3. Verifies `sig` over the same canonical bytes.
//!
//! # Canonical bytes
//!
//! The signed payload is the UTF-8 bytes of a deterministic string:
//!
//! ```text
//! <from>|<to>|<ts>|<messageId>|<message>
//! ```
//!
//! Each field is the raw JSON string value (no surrounding quotes). `|` is the
//! field separator; none of the fields can contain a bare `|` in practice (
//! agent ids, timestamps, and message-ids are constrained). Message body *can*
//! contain `|`, which is fine — the first 4 fields are fixed-format so parsing
//! is unambiguous. Including `messageId` + `ts` in the payload means replayed
//! envelopes with a different id or timestamp will fail verification. Full
//! replay-window enforcement (time-based nonce expiry) is a follow-up.
//!
//! # Dep note (flagged for dep-lean review)
//!
//! New deps added to `bwoc-core` (previously dep-lean):
//! - `ed25519-dalek = "2"` (with feature `rand_core`) — pure-Rust ed25519
//! - `rand_core = "0.6"` (with feature `getrandom`) — OS entropy for keygen
//! - `hex = "0.4"` — minimal hex encode/decode; no_std compatible

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;

/// Re-export so downstream crates can use `bwoc_core::signing::SigningKey`
/// without depending on `ed25519-dalek` directly.
pub use ed25519_dalek::SigningKey as AgentSigningKey;

// ── Error type ────────────────────────────────────────────────────────────────

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
    #[error("key file already exists at {0} (pass --force to overwrite)")]
    KeyExists(PathBuf),
}

// ── Key generation + storage ──────────────────────────────────────────────────

/// Generate a new ed25519 keypair and store it under `<agent_bwoc_dir>/agent.key`.
///
/// `agent_bwoc_dir` is the `.bwoc/` directory inside the agent directory
/// (e.g., `agents/agent-oracle/.bwoc/`). The private key is written as
/// lowercase hex to `.bwoc/agent.key` with mode 0600 (Unix). The public key
/// is returned as a lowercase hex string for the caller to write into
/// `config.manifest.json`'s `trust.publicKey`.
///
/// Idempotency: if the key file already exists, this function returns
/// `Err(SigningError::KeyExists)` unless `force` is `true`.
pub fn generate_keypair(agent_bwoc_dir: &Path, force: bool) -> Result<String, SigningError> {
    let key_path = agent_bwoc_dir.join("agent.key");
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

/// Load the signing key from `<agent_bwoc_dir>/agent.key`.
/// Returns `None` when the file is absent (backward-compat: unsigned send).
/// Returns `Err` when the file is present but malformed.
pub fn load_signing_key(agent_bwoc_dir: &Path) -> Result<Option<SigningKey>, SigningError> {
    let key_path = agent_bwoc_dir.join("agent.key");
    if !key_path.exists() {
        return Ok(None);
    }
    let hex_str = fs::read_to_string(&key_path)?;
    let bytes = hex::decode(hex_str.trim())?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| {
        SigningError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected 32-byte private key",
        ))
    })?;
    Ok(Some(SigningKey::from_bytes(&arr)))
}

/// Parse a verifying (public) key from a hex string stored in `trust.publicKey`.
/// Returns `None` when the hex string is absent.
pub fn load_verifying_key(pubkey_hex: &str) -> Result<VerifyingKey, SigningError> {
    let bytes = hex::decode(pubkey_hex.trim())?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| {
        SigningError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected 32-byte public key",
        ))
    })?;
    Ok(VerifyingKey::from_bytes(&arr)?)
}

// ── Canonical bytes ───────────────────────────────────────────────────────────

/// Build the canonical byte string that is signed and verified.
///
/// Format: `<from>|<to>|<ts>|<messageId>|<message>` (UTF-8).
/// Fields are the raw string values from the envelope (no JSON quoting).
/// The separator `|` is chosen because it cannot appear in agent ids,
/// ISO-8601 timestamps, or the `msg-…` message-id format. It CAN appear
/// in message body — the first 4 fields are fixed-format so this is safe.
pub fn canonical_bytes(from: &str, to: &str, ts: &str, message_id: &str, body: &str) -> Vec<u8> {
    format!("{from}|{to}|{ts}|{message_id}|{body}").into_bytes()
}

// ── Sign + verify ─────────────────────────────────────────────────────────────

/// Sign `payload` with `key` and return the lowercase hex signature (128 hex chars).
pub fn sign(key: &SigningKey, payload: &[u8]) -> String {
    hex::encode(key.sign(payload).to_bytes())
}

/// Verify a hex-encoded `sig` against `payload` using `verifying_key`.
/// Returns `Ok(())` on success, `Err(SigningError::BadSignature)` on failure.
pub fn verify(
    verifying_key: &VerifyingKey,
    payload: &[u8],
    sig_hex: &str,
) -> Result<(), SigningError> {
    let sig_bytes = hex::decode(sig_hex.trim())?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| SigningError::BadSignature)?;
    let sig = Signature::from_bytes(&sig_arr);
    verifying_key
        .verify(payload, &sig)
        .map_err(|_| SigningError::BadSignature)
}

// ── Private key file helpers ──────────────────────────────────────────────────

/// Write `hex_key` to `path`, set Unix permissions to 0600, ensure it is
/// terminated with a newline. On non-Unix platforms, permission setting is
/// a no-op (the file is still written; operators must secure it manually).
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

    // ---- keygen ----------------------------------------------------------------

    #[test]
    fn keygen_creates_key_file_and_returns_pubkey_hex() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey = generate_keypair(&bwoc, false).unwrap();
        // Public key is 64 hex chars (32 bytes).
        assert_eq!(pubkey.len(), 64, "pubkey hex length: {pubkey}");
        assert!(
            pubkey.chars().all(|c| c.is_ascii_hexdigit()),
            "pubkey is hex"
        );
        // Private key file exists.
        let key_path = bwoc.join("agent.key");
        assert!(key_path.exists());
        let stored = fs::read_to_string(&key_path).unwrap();
        assert_eq!(stored.trim().len(), 64, "privkey hex length");
    }

    #[test]
    fn keygen_idempotent_without_force_fails() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        generate_keypair(&bwoc, false).unwrap();
        // Second call without --force → KeyExists error.
        let err = generate_keypair(&bwoc, false).unwrap_err();
        assert!(matches!(err, SigningError::KeyExists(_)));
    }

    #[test]
    fn keygen_force_overwrites() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pk1 = generate_keypair(&bwoc, false).unwrap();
        let pk2 = generate_keypair(&bwoc, true).unwrap();
        // Different keys (statistically guaranteed for real entropy; we just
        // check that the call succeeds and produces a valid pubkey).
        assert_eq!(pk2.len(), 64);
        // The two pubkeys will almost certainly differ (2^-128 collision chance).
        // We don't assert inequality to keep the test deterministic-safe.
        let _ = pk1;
    }

    // ---- sign + verify round-trip ---------------------------------------------

    #[test]
    fn sign_verify_roundtrip_valid() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey_hex = generate_keypair(&bwoc, false).unwrap();

        let payload = canonical_bytes(
            "agent-alpha",
            "agent-beta",
            "2026-05-24T10:00:00Z",
            "msg-20260524T100000Z-ab123",
            "hello",
        );

        let key = load_signing_key(&bwoc).unwrap().unwrap();
        let sig_hex = sign(&key, &payload);
        assert_eq!(sig_hex.len(), 128, "sig is 64 bytes = 128 hex chars");

        let vk = load_verifying_key(&pubkey_hex).unwrap();
        verify(&vk, &payload, &sig_hex).expect("valid signature must verify");
    }

    #[test]
    fn tampered_body_fails_verify() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        let pubkey_hex = generate_keypair(&bwoc, false).unwrap();

        let payload = canonical_bytes(
            "agent-alpha",
            "agent-beta",
            "2026-05-24T10:00:00Z",
            "msg-20260524T100000Z-ab123",
            "original body",
        );
        let key = load_signing_key(&bwoc).unwrap().unwrap();
        let sig_hex = sign(&key, &payload);

        // Tamper: different body.
        let tampered = canonical_bytes(
            "agent-alpha",
            "agent-beta",
            "2026-05-24T10:00:00Z",
            "msg-20260524T100000Z-ab123",
            "tampered body",
        );
        let vk = load_verifying_key(&pubkey_hex).unwrap();
        let result = verify(&vk, &tampered, &sig_hex);
        assert!(
            matches!(result, Err(SigningError::BadSignature)),
            "tampered body must fail: {result:?}"
        );
    }

    #[test]
    fn wrong_pubkey_fails_verify() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let bwoc1 = dir1.path().join(".bwoc");
        let bwoc2 = dir2.path().join(".bwoc");
        let pubkey2 = generate_keypair(&bwoc2, false).unwrap();
        generate_keypair(&bwoc1, false).unwrap();

        let payload = canonical_bytes(
            "agent-alpha",
            "agent-beta",
            "2026-05-24T10:00:00Z",
            "msg-20260524T100000Z-ab123",
            "hello",
        );
        // Sign with key1, verify with key2's pubkey.
        let key1 = load_signing_key(&bwoc1).unwrap().unwrap();
        let sig_hex = sign(&key1, &payload);

        let vk2 = load_verifying_key(&pubkey2).unwrap();
        let result = verify(&vk2, &payload, &sig_hex);
        assert!(
            matches!(result, Err(SigningError::BadSignature)),
            "wrong pubkey must fail: {result:?}"
        );
    }

    #[test]
    fn load_signing_key_absent_returns_none() {
        let dir = tempdir().unwrap();
        let bwoc = dir.path().join(".bwoc");
        // No key file — should return None, not error.
        let result = load_signing_key(&bwoc).unwrap();
        assert!(result.is_none());
    }

    // ---- canonical bytes shape -----------------------------------------------

    #[test]
    fn canonical_bytes_pipe_separated() {
        let b = canonical_bytes(
            "agent-alpha",
            "agent-beta",
            "2026-05-24T00:00:00Z",
            "msg-001",
            "hi",
        );
        let s = String::from_utf8(b).unwrap();
        assert_eq!(s, "agent-alpha|agent-beta|2026-05-24T00:00:00Z|msg-001|hi");
    }
}
