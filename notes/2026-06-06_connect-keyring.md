# 2026-06-06 — Connector keyring token resolution

Completes the token story the architect chose ("keyring default, env fallback").
PR1 shipped env-only as the interim path; this adds the OS keyring where it
earns its weight.

## What changed

- **`bwoc-connect` token resolution** — `resolve_token(platform, agent_dir,
  env)`: OS keyring first (service `bwoc/<platform>`, account = agent-dir
  basename) **on macOS/Windows**, then the platform env var (`TELEGRAM_BOT_TOKEN`
  / `DISCORD_BOT_TOKEN`). A missing/locked/absent keyring or empty entry falls
  through to env; an absent token's error names both sources.
- **`keyring_lookup` is cfg-gated**: native store on macOS (`apple-native`) /
  Windows (`windows-native`); a `None` stub on every other target. Linux is
  env-only.

## Decisions

- **Linux = env-only (Mattaññutā).** Secret Service on Linux means either
  `sync-secret-service` → `dbus-secret-service` → `libdbus-sys` (a system C lib;
  ubuntu CI has no `dbus-1.pc` → build fails — confirmed on the first #224 run),
  or `async-secret-service` → zbus → a second async runtime bridged into our
  tokio (deadlock-prone). That's a lot of weight + risk for a feature the actual
  deployment target (headless bemind server) can't use — it has no Secret Service
  daemon, so it falls back to the env var regardless. So Linux stays env-only;
  the env var is the fallback on every platform anyway. "The smaller spec beats
  the more complete one."
- **keyring-first, env-fallback, never fatal** — matches the architect's call and
  the `CredentialBroker` posture; quarantined in `bwoc-connect`.

## Bugs surfaced and fixed

- First attempt wired Linux to `sync-secret-service` + `crypto-rust` believing it
  was pure-Rust. It is **not** — it pulls `libdbus-sys` (system C lib). CI caught
  it (ubuntu build + clippy failed at the `libdbus-sys` build script: `dbus-1`
  not in pkg-config). Fixed by dropping the Linux keyring backend (env-only).

## Status / deferred

- macOS/Windows keyring done (macOS verified locally; Windows via CI). Linux
  env-only by design. **Last connector follow-up — Discord gateway RESUME — is
  deliberately deferred** (YAGNI: fresh-IDENTIFY reconnect already works; RESUME
  is an unverifiable optimization on the integration-untested edge). With that,
  bwoc-connect is complete.

## Related

- `crates/bwoc-connect/src/main.rs`, `crates/bwoc-connect/Cargo.toml`
