# 2026-06-04 — Windows named-pipe daemon

`bwoc-agent --serve` now runs on Windows over a named pipe, replacing the
exit-2 stub — the last code item under Phase 2 "Remaining for ship". Developed
blind on macOS via `cargo check --target x86_64-pc-windows-msvc`; behaviour is
exercised by a protocol roundtrip test on the windows-latest CI leg.

## What changed

- **`bwoc-core::ipc`** — `pipe_name(agent_dir)`: deterministic per-agent pipe
  name (`bwoc-agent-<fnv1a-16hex>` over the canonicalized dir). Unix
  namespaces the endpoint by file path; the Windows pipe namespace is global,
  so server and clients derive the same name independently — no rendezvous
  file needed (one is still written to `.bwoc/agent.pipe` for humans/doctor).
  Dependency-free (inline FNV-1a) so core stays lean.
- **`bwoc-agent`**: `serve_loop` split into a transport-independent
  `serve_core` (PID file, inbox watch + cursor, trust gating, Saṅgha task
  watch, accept-or-idle poll loop, cleanup) and thin per-platform shells —
  Unix keeps the exact `agent.sock` contract; Windows builds an `interprocess`
  named-pipe listener (nonblocking accept) and passes the same closure shape.
  `handle_client` is generic over `Read + Write`; `trust`/`task_watch` mods
  un-gated (they were always portable). Fallback stub kept for
  `not(any(unix, windows))`.
- **`bwoc-cli` clients**: `livecheck::pipe_request` (one-line request/reply
  over the pipe) powers Windows `query_uptime` (STATUS) and `ping`;
  `signal_zero_alive` uses `tasklist /FI "PID eq N"`; `stop`'s escalation
  mirrors Unix — pipe STOP → `taskkill /PID` (polite) → `taskkill /F` —
  with the shared `wait_for_exit` poll un-gated.
- Deps: `interprocess = "2.2"` under `[target.'cfg(windows)'.dependencies]`
  of bwoc-agent + bwoc-cli only — Unix builds never compile it.

## Decisions

- **`interprocess` cfg(windows)-only, Unix untouched (Anattā on the stable
  path).** Could have unified both platforms on `interprocess`, but the Unix
  UDS contract (`agent.sock` path, `nc -U` debugging) is documented and load-
  bearing; zero-risk beats DRY here. The shared `serve_core` still removes the
  ~150-line duplication that mattered.
- **`tasklist`/`taskkill` shell-outs over `windows-sys` FFI** — liveness and
  kill are human-cadence operations; a dependency-free shell-out beats unsafe
  FFI written blind from macOS.
- **No read-timeout on the Windows client streams** (the sync pipe API has
  none): a hung-but-connected daemon would block a client. Accepted for v1 —
  a healthy daemon answers in microseconds, and Unix keeps its timeouts.
- **`&mut stream` in `handle_client`** — `BufReader::new(&stream)` needs
  `&S: Read`, which only concrete types provide; the generic bound version
  sent rustc into an E0275 candidate-enumeration overflow (via a macOS
  `dispatch2` impl). `&mut S: Read` follows from `S: Read` by blanket impl.

## Gotchas surfaced

- **Homebrew Rust shadows rustup on this machine** — `rustup run stable`
  did not put the toolchain first on PATH, so `--target` checks hit "can't
  find crate for core" despite the std being installed. Workaround: prepend
  `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin` explicitly.
- **macOS `SUN_LEN`** — a daemon smoke test from `$TMPDIR` fails to bind
  (`path must be shorter than SUN_LEN`, ~104 bytes): pre-existing Unix
  constraint, not a regression; smokes use `/tmp/...` instead.

## Verification

- Unix: full lifecycle smoke (start → PONG → STATUS uptime → stop → sock+pid
  cleaned) against the refactored `serve_core`; `cargo test -p bwoc-agent`
  (27) + `-p bwoc-core ipc` (3); clippy `-D warnings` clean across the three
  crates.
- Windows: `cargo check --target x86_64-pc-windows-msvc --all-targets` clean;
  `named_pipe_roundtrip_ping_status_stop` (PING/STATUS/STOP over a real pipe
  against the real `handle_client`) runs on the windows-latest CI leg.

## Related

- `crates/bwoc-core/src/ipc.rs`, `crates/bwoc-agent/src/main.rs`,
  `crates/bwoc-cli/src/{livecheck,ping,stop}.rs`
- ROADMAP Phase 2 "Remaining for ship" (EN+TH) updated.
