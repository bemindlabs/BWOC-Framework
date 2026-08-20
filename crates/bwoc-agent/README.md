# bwoc-agent

The per-agent daemon shipped with each incarnated [BWOC](../../README.md) agent.

A native single binary for **macOS · Linux · Windows**. Run inside an incarnated agent directory, it reads `config.manifest.json` and either prints a liveness banner and exits, or (with `--serve`) runs as a long-lived daemon: control endpoint, inbox polling with trust + signature gating, Saṅgha task watch, and supervision of the network-facing child processes. **Dep-quarantine is load-bearing** — this crate has no WebSocket, TLS, or HTTP dependencies. Anything that touches the network ([`bwoc-connect`](../bwoc-connect/), `bwoc-gateway-recv`, [`bwoc-harness`](../bwoc-harness/)) is spawned as a child and merely supervised here. Depends on [`bwoc-core`](../bwoc-core/) for shared types and [`bwoc-signing`](../bwoc-signing/) for envelope verification.

## Scope

- **`main`** — arg handling (`--serve`, `--version`, `--help`), the liveness banner, and `serve_core`: the transport-independent daemon loop. Writes `.bwoc/agent.pid`, serves a line-based protocol (`PING` → `PONG`, `STATUS` → `OK uptime_secs=N pid=P`, `STOP`), polls the inbox on idle ticks, and exits cleanly on SIGTERM/SIGINT. Transport is a Unix domain socket at `.bwoc/agent.sock` (debuggable with `nc -U`) or, on Windows, a named pipe recorded in `.bwoc/agent.pipe`.
- **`trust`** — Kalyāṇamitta-7 refusal logic over new inbox envelopes: `Pass` / `Warn` / `Refuse`, with refusals appended to `.bwoc/inbox.refusals.jsonl` (the original envelope in `inbox.jsonl` is never deleted). Also holds ed25519 signature verification (`BWOC_SIGNING_MODE`, default `enforce`) and a `ReplayGuard` over `(from, nonce)` plus a timestamp freshness window.
- **`task_watch`** — watches the shared task lists of every team the agent belongs to and announces newly-claimable tasks. Optional auto-claim and tmux wakeup.
- **`warm`** — keeps one resident `bwoc-harness --headless` process loaded so claimed team tasks run **trusted** without a cold start. Refuses ambient (non-harness) backends; skips `requires_plan` tasks, which need lead approval.
- **`autoprocess`** — answers gateway-relayed messages by keeping one **untrusted**, read-only `bwoc-harness --chat` session per remote sender (idle-reaped, so a back-and-forth reuses its context), auto-denying every permission request and replying via `bwoc send`. Deliberately never shares a process with `warm` — one untrusted turn taints a session permanently.
- **`connectors`** — spawns and keeps alive `bwoc-connect` when the agent declares an enabled connector, with backoff on crash-loop.
- **`gateway`** — same supervision for `bwoc-gateway-recv`, which dials the relay and appends inbound envelopes into `.bwoc/inbox.jsonl`.
- **`i18n`** — Project Fluent bundles from `locales/{en,th}/agent.ftl`; locale from `BWOC_LANG`, falling back to `$LANG` then `en`.

## Usage

Normally started by `bwoc start <agent>`, which spawns `bwoc-agent --serve` unless `--no-daemon` is passed. On a root-only host, `bwoc agent run <agent> --as-user <user>` drops privilege first and then runs the same command. Directly, from inside an incarnated agent directory:

```bash
bwoc-agent            # print the liveness banner and exit
bwoc-agent --serve    # run as daemon; blocks until SIGTERM / SIGINT
```

Talk to a running daemon over its socket:

```bash
printf 'STATUS\n' | nc -U .bwoc/agent.sock
```

Opt-in environment flags: `BWOC_TRUST_GATING`, `BWOC_SIGNING_MODE`, `BWOC_WARM`, `BWOC_AUTO_CLAIM`, `BWOC_TASK_WAKEUP`, `BWOC_TASK_POLL_SECS`, `BWOC_LANG`.

## Status

Working daemon, not a stub. The control protocol, inbox polling with persisted cursor, trust and signature gating, task watch, warm execution, auto-process, and child supervision all ship today; `bwoc help daemon` documents operator-facing behavior. See [`SIGNING.en.md`](../../docs/en/SIGNING.en.md) and [`ARCHITECTURE.en.md`](../../docs/en/ARCHITECTURE.en.md).

## License

[MIT](../../LICENSE).
