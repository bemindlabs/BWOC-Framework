# 2026-06-06 — Connector keyring token resolution

Completes the token story the architect chose ("keyring default, env fallback").
PR1 shipped env-only as the interim headless path; this adds the OS keyring.

## What changed

- **`bwoc-connect` token resolution** — `resolve_token(platform, agent_dir,
  env)`: OS keyring first (service `bwoc/<platform>`, account = agent-dir
  basename), then the platform env var (`TELEGRAM_BOT_TOKEN` /
  `DISCORD_BOT_TOKEN`). A missing/locked keyring or empty entry falls through to
  env; an absent token's error names both sources.
- **Per-OS `keyring` deps** (target-gated) so the crate compiles everywhere:
  macOS `apple-native`, Windows `windows-native`, Linux `sync-secret-service` +
  `crypto-rust` (pure-Rust zbus + rust-crypto — no system libdbus/openssl). Still
  quarantined in `bwoc-connect`.

## Decisions / verification

- **keyring-first, env-fallback, never fatal.** Matches the architect's call and
  the existing `CredentialBroker` posture; the headless bemind host keeps using
  the env var (no keyring there) with zero friction.
- **Cross-platform feature flags**: the macOS build is verified locally
  (`apple-native` + the usage compile). The Linux/Windows feature names can't be
  cross-checked from macOS (ring's C cross-compile blocks `cargo check --target`,
  same as the windows harness check), so **CI validates the ubuntu/windows
  builds** — and the env fallback makes the keyring non-load-bearing at runtime,
  so a wrong flag fails the build (caught) rather than misbehaving in prod.

## Status / next

- Keyring done (pending CI on the other two OSes). **Last remaining connector
  follow-up: Discord gateway RESUME** (reconnect without a full re-IDENTIFY) —
  then bwoc-connect is complete.

## Related

- `crates/bwoc-connect/src/main.rs`, `crates/bwoc-connect/Cargo.toml`
