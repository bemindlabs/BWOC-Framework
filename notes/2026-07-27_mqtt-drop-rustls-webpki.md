# 2026-07-27 — Drop rumqttc's unused rustls feature (fixes #389)

Issue #389 concluded the `rustls-webpki` 0.102.8 HIGH (GHSA-82j2-j2ch-gfr8) was "blocked upstream" because `rumqttc` directly pins `rustls-webpki = "^0.102.8"` and the fix only exists in the API-breaking 0.103 line. That conclusion missed the simpler exit: **`bwoc-mqtt` uses no TLS at all**, so the entire rustls tree is dead weight.

## What changed
`crates/bwoc-mqtt/Cargo.toml`: `rumqttc = "0.24"` → `rumqttc = { version = "0.24", default-features = false }`. rumqttc's `default = ["use-rustls"]` was the *sole* thing dragging in `rustls-webpki` (+ `rustls`, `tokio-rustls`, `rustls-native-certs`, `rustls-pemfile`). bwoc-mqtt constructs a plaintext `MqttOptions` (grep: no `Transport::Tls`, no `rustls`, no 8883), so dropping the feature removes the whole subtree — `rustls-webpki 0.102.8` is gone from the lockfile (only an unrelated, non-vulnerable 0.103.13 remains).

## Verification
`cargo test -p bwoc-mqtt` 12/12 pass; fmt + clippy clean; `trivy fs --severity HIGH,CRITICAL --ignore-unfixed` on the framework now reports zero (both the quinn-proto fix from #388 and this webpki removal). Fewer deps compiled, not more.

## Decisions
- **Drop the feature, not force webpki 0.103.** Forcing 0.103 breaks rumqttc's compilation; disabling the unused TLS feature is strictly smaller and removes the attack surface entirely rather than patching it.
- If bwoc-mqtt ever needs broker TLS, re-enable `use-rustls` (by then rumqttc will hopefully carry webpki 0.103) or add `use-native-tls`.

## Related
- Fixes #389 (framework side). Same one-line fix applies to `bwoc-devices` (`bwoc-device-linux`, optional `mqtt` feature) — separate PR. Supersedes the "blocked upstream" note there.
