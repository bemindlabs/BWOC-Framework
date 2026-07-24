# Changelog

All notable changes to BWOC are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/). See [`VERSION.md`](VERSION.md) for the current version and phase status.

## [Unreleased]

### Added

- **`audit` kind now spans ISO / IEC / IEEE** — added `audit-iso-iec-ieee-29148`, a Requirements-Engineering audit against **ISO/IEC/IEEE 29148:2018** (the first jointly ISO + IEC + IEEE standard in the set; supersedes IEEE 830/1233/1362), reusing the shared attestation runtime. Its 7 criteria cover StRS / SyRS-SRS, individual + set requirement characteristics, verifiability, bidirectional traceability, and requirements management. The `audit` kind's framing + designations were generalised to the three standards bodies (ISO 9001 · ISO/IEC 20000-1/27001/29110 · ISO/IEC/IEEE 29148) in `docs/{en,th}/PLUGINS`.
- **`soul` framework skill** (`modules/skills/soul/`, `domain/identity`): hold an agent's enduring core — values, voice, principles, boundaries — consistently across every task so it is recognizably *itself*. `embody` (act from the core, expressed not announced) + `reflect` (catch drift + realign, correcting ego-free). The BWOC paradox: a consistent stream of character (Sīla, Adhiṭṭhāna) held *without* clinging to a fixed ego (Anattā) — and the one skill whose teardown is deliberately not a release (the core persists across tasks). L1.
- **`accounting-api` workflow plugin** (`modules/plugins/workflow/accounting-api/`): a `workflow`-kind adapter over the Bemind Accounting Open API (v2.3.2) — `report` (read `/reports/<name>` — pnl / balance-sheet / cashflow / trial-balance / vat / …), `bill-create` + `bill-update` (the 2-step `/purchase-docs` `POST → PATCH` bill flow, `purchases:write`), and `expense-create` (`/expenses`, `expenses:write`). Every write auto-posts a double-entry GL entry server-side. Operator API key (bound to one seller) resolves from `BWOC_ACCOUNTING_KEY` / `.bwoc/secrets/accounting-key` (never committed); a User-Agent header is required. First slice — the `bwoc accounting` CLI carrying the write-verb operator-confirm gate is a follow-up. Grounded in the app's live OpenAPI.

## [v2026.7.23-0] — 2026-07-23 — 2.36.0

### Added

- **`ai-loop-engineer` framework skill** (`modules/skills/ai-loop-engineer/`, `domain/autonomy`): engineer autonomous agent loops (`perceive → act → observe`) that converge, stay bounded, and escalate — `design_loop` (iteration unit · stop condition = done-when + max-iterations + budget ceiling · guardrails · escalation gate) + `tune_loop` (fix non-convergence / runaway cost / oscillation / silent failure on a live loop). Distinct from `ai-dlc` (human-steered development lifecycle); this builds the *agent's own* iteration engine. Maps to Appamāda (heedful), Mattaññutā (bounded), Anattā (stop/pivot), + deferred-control at the irreversible gate. L1.

### Fixed

- **Skill SPEC wikilink depths** — the docs cross-links in the skill SPECs pointed at non-existent paths (`PHILOSOPHY.en` lives under `modules/agent-template/docs/en/`, not repo-root `docs/en/`; `SKILLS.en` is three levels up from `modules/skills/<name>/`; the agent-template `AGENTS` link had a doubled `modules/` segment). Corrected across the affected skill SPECs (the reference `worktree-discipline` shared the same off-by-one). Cosmetic — Obsidian resolves by note name — but the paths now match the repo layout.

## [v2026.7.22-1] — 2026-07-22 — 2.35.0

### Added

- **Role skill set — five framework skills** (`modules/skills/`): profession/role skills extending the craft library — **product-manager** (discover/define), **systems-engineer** (architect_system/assure_reliability), **software-engineer** (design_component/review_code), **data-engineer** (build_pipeline/ensure_data_quality), **data-scientist** (analyze/build_model). Each names its distinction from the adjacent craft (e.g. software-engineer's design+review brackets engineering's implement+harden; product-manager's what/why vs manager's how), maps to a framework principle, and is environment-neutral, L1.
- **`ai-dlc` framework skill** (`modules/skills/ai-dlc/`, `domain/methodology`): the AI-Driven Development Life Cycle as a repeatable practice — `plan_increment` (elaborate a human intent into an agreed, right-sized plan; stop for approval before build) + `execute_increment` (drive build → verify → document → operate, pausing at each bounded/irreversible gate for the framework's operator-confirm). The *meta-skill* that sequences the craft skills (`manager`/`engineering`/`auditor`/`documenter`) across a lifecycle, grounding AI-DLC's "agent drives, human steers at the gates" in BWOC's own `Sīla` gates + deferred-control + `uppāda/ṭhiti/vaya` arc. L1.
- **Craft skill library — ten framework skills** (`modules/skills/`): a library of reusable craft/persona capabilities an agent enables per role — **writer** (draft/revise), **illustrator** (compose_prompt/generate), **manager** (plan/delegate), **counselor** (listen/advise), **auditor** (audit/verify_finding), **engineering** (implement/harden), **mathematics** (derive/check_result), **physics** (model/estimate), **documenter** (document/sync), **lawyer** (review/flag_risk). Each maps to a Buddhist principle the framework already uses (Sammā-vācā, Yoniso Manasikāra, Sīla, the Brahmavihāra…), exposes two operations, is environment-neutral (no hardcoded backend/model/vendor), declares `maturity = L1`, and cross-links its siblings. `counselor` and `lawyer` carry a humility clause routing weighty decisions to a qualified human. Enable per agent with `bwoc skill enable`.
- **`second-brain` + `server-rag` framework skills** (`modules/skills/`): two sibling `domain/knowledge` skills encoding "consult the fleet's existing knowledge before re-deriving" (Remember-first, fleet-wide). `second-brain` queries the fleet's harvested knowledge **graph** (`query_brain` — by term across every workspace/repo/commit/PR/issue/memory/note; `refresh_brain` re-harvests); `server-rag` asks the self-hosted **semantic RAG** (`ask_rag` — natural-language question → answer + sources; `refresh_rag` re-ingests). Both are read-first (no operator-confirm gate on the local-only refresh) and environment-neutral (operator-configured `<secondBrainRoot>` / `<ragQueryUrl>`). Enable per agent with `bwoc skill enable`.

## [v2026.7.22-0] — 2026-07-22 — 2.34.0

### Added

- **`gws-slides` — Google Slides read + in-place write**: a new `gws` per-service plugin and `bwoc gws slides` verbs — `get` (presentation title + slide ids), `batch-update` (`presentations.batchUpdate` — the general write path), and `replace-all-text` (convenience). Reuses the operator-confirm gate; requires the `presentations` OAuth scope. **Completes the Docs/Sheets/Slides series** — the `gws` kind now has three write-capable Google Workspace editor services.
- **`gws-sheets` — Google Sheets read + values write**: a new `gws` per-service plugin and `bwoc gws sheets` verbs — `get` (spreadsheet title + tab list), `values-get` (read a cell range), and the gated writes `values-update` (`spreadsheets.values.update` — overwrite a range) and `values-append` (`spreadsheets.values.append` — append rows). Reuses the operator-confirm gate from `gws-docs` (default No, interactive `y/N`, `--yes` for headless, `--json` requires `--yes`). Writes use `valueInputOption=USER_ENTERED`. Requires the `spreadsheets` OAuth scope; sources credentials from `gws-auth`. Second slice of the Docs/Sheets/Slides series.
- **`gws-docs` — Google Docs read + in-place write** (issue #354): a new `gws` per-service plugin and `bwoc gws docs` verbs — `get` (`documents.get` — metadata + bounded body text), `batch-update` (`documents.batchUpdate` — the general write path), and `replace-all-text` (convenience). This is the **first write-capable `gws` service**, so it introduces the kind's use of the **operator-confirm gate**: the write verbs default to No, prompt `y/N` interactively, require `--yes` for headless agents (and with `--json`), and report "no change" on refusal — the gate lives at the `bwoc gws` CLI boundary, not the plugin. Requires the `documents` OAuth scope (read+write); sources credentials from the sibling `gws-auth`. Google Docs `gws-sheets` / `gws-slides` follow as separate slices.

## [v2026.7.20-0] — 2026-07-20 — 2.33.0

### Added

- **`bwoc run --workdir <dir>`** — opt-in un-jail for cross-project headless tasks. By default `bwoc run` still spawns the agent in its own directory (the safe, blast-radius-minimal jail), so a headless agent can't touch shared workspace files (`projects/`, `wiki/`). Passing `--workdir` runs from another directory — relative paths resolve against the workspace root (`--workdir .` = the workspace root) — so a task that legitimately needs cross-project scope can edit those files. The resolved path must be an existing directory **inside** the workspace root (canonicalized; `..` escapes are refused). The widened dir applies to every backend: it becomes the process cwd for ambient backends (`claude -p`) and the harness `--workdir` FS-jail root for harness backends (the agent's `config.manifest.json` is always still read from its own directory).

## [v2026.7.19-0] — 2026-07-19 — 2.32.1

### Fixed

- **`--chat` / `--headless` now resolve the `primaryModel: "auto"` sentinel** (issue #347, PR #348): the batch `run()` path resolved `auto` via `model_select::resolve_auto` before validating and running, but the chat path validated and looped on the raw `"auto"` string, so an `auto`-model agent couldn't chat (validation errored, or with `--skip-model-check` the provider rejected the literal `"auto"`). `run_chat_mode` now resolves the sentinel the same way, before `validate_model` and before building the session. Surfaced by the bwoc-vscode-extension chat feature.

## [v2026.7.18-0] — 2026-07-18 — 2.32.0

### Added

- **`bwoc agent run --as-user <user> <agent>`** (issue #322): launch an agent session as an unprivileged user on a root-only VPS, where `--dangerously-skip-permissions` refuses to run as root. From root it validates the target user, drops privilege via `runuser`, and launches in the agent's own directory (default `bwoc-agent --serve`, or an explicit `-- <cmd>` like a remote-control session). Conservative by design — Unix-only, root-required, and it does **not** create the user or `chown` the workspace (it warns if the agent dir isn't owned by the user); those one-time steps stay in the new [`DEPLOYMENT`](docs/en/DEPLOYMENT.en.md) guide (also added this cycle).

- **`bwoc doctor` manifest-vs-reality checks** (issue #323): three new per-agent audits on top of the existing environment/hygiene sweeps — `manifest:` (config.manifest.json present, valid JSON, no leftover `{{placeholder}}` tokens), `agent key:` (`.bwoc/agent.key` must be owner-only — no group/other access — when present; `--auto` chmods to 600), and `models:` (for `backend = ollama` agents, verify `primaryModel`/`fallbackModel` are actually installed via Ollama `/api/tags` at the agent's own `baseUrl` endpoint — the pre-`ollama rm` safety check). A pinned tag (`gemma4:9b`) requires an exact match; a tagless name matches any tag. Unreachable/`https` endpoint → per-agent WARN; HTTP done over `std::net` (no new dep). FAILs set exit 2; runnable offline / in CI.

- **`bwoc receipts` — read receipts** (issue #299, second half): a sender can now ask "was my message consumed?" `bwoc triage` records the source `messageId` on each receipt it writes, and `bwoc receipts` queries those logs fleet-wide — filter `--message-id` / `--from` / `--agent`, table or `--json`. A message-id with no receipt is an explicit "not consumed yet." Completes #299 (the delivery/dedup half shipped in #311). Cross-machine ack-back via the gateway remains a transport follow-up.

- **iMessage chat connector** (`bwoc-connect imessage`, issue #229) — a fourth connector, **macOS-only and free**: receive by polling the local `~/Library/Messages/chat.db` (read-only), send via `osascript` → Messages.app. DM-first, non-streaming. Handles (phone/email) ride the existing `i64` allow-list seam via `imessage::hash_id` + an `[imessage] allow_handles` config block (option B). Confined to a `cfg(target_os = "macos")` transport — hard-errors on other platforms; the daemon supervises it via the `KNOWN` table. No token (drives the local app); needs Full Disk Access + Automation TCC grants. Caveats: the agent speaks as the Mac's own Apple ID (no bot identity), and automating Messages is against Apple's ToS (personal use). `docs/{en,th}/CONNECTORS.md` updated.

- **Warm task execution in `bwoc-agent --serve`** (issue #301, PR B of 3, opt-in `BWOC_WARM=1`, default off): when auto-claim wins a Saṅgha task, it runs in a **resident `bwoc-harness --headless`** (lazy-spawned, idle-reaped after 600s, respawned on demand) instead of cold-starting / tmux-waking — eliminating per-task backend cold-start. Trusted task path only (`Principal::LocalOperator`); untrusted gateway input keeps its existing read-only `--chat` + auto-deny path. Confined backends only (ambient `cli` refused); `requires_plan` tasks are skipped to preserve the Pavāraṇā lead-approval gate. Completion is recorded via `bwoc task complete`.
- **`bwoc-harness --headless`** — a served/warm session mode (issue #301, PR A of 3): the same multi-turn `chat_proto` loop as `--chat`, but driven by a machine frontend instead of a human. `ask`-mode tools auto-approve so a turn never blocks on a permission prompt, while guardrails, policy `deny` rules, and the worktree sandbox still confine it. Conflicts with `--unrestricted` (which would lift that sandbox). This is the standalone warm loop; daemon supervision (resident process, lazy spawn + idle-exit, inbox routing) follows in PR B.

### Fixed

- **Env scrub before spawning MCP servers, made Windows-safe** (issue #336): `StdioTransport::spawn` now `env_clear`s and re-injects only the `scrub_env` allowlist, so a third-party MCP server no longer inherits `ANTHROPIC_API_KEY`/`OPENROUTER_API_KEY`/every `*TOKEN*/*SECRET*/*KEY*`. The allowlist is now matched case-insensitively and carries the Windows essentials (`Path`, `SystemRoot`, `COMSPEC`, …) so `env_clear`'d children still start on Windows — fixing `run_command`/audit there too.
- **Bounded Anthropic `complete()`** (issue #338): the non-streaming path now has a per-call `tokio::time::timeout` (shared `REQUEST_TIMEOUT`, 120s), so a server that stalls before the first byte surfaces as `TransientProvider` and retries instead of hanging the turn forever. `stream()` is untouched.
- **`bwoc list` / `fleet health|status` error on an invalid `--workspace`** (issue #339) instead of silently reporting an empty, healthy-looking fleet with exit 0 — a bogus path now fails loud with exit 2.
- **Warn when the OS sandbox/jail is unavailable** on unsupported targets (issue #337): the `NoopOsSandbox` / `JailStatus::Unavailable` fallbacks now emit a one-time warning (Windows relies on path-confinement only) instead of degrading silently.
- **t6 `setrlimit` bomb-containment tests now run in CI** (issue #335): `BWOC_T6_RUN_BOMBS=1` is exported for a dedicated serial step, so containment is proven by an actual fork/mem/cpu/fsize bomb, not just a value snapshot.
- **Hardened the parallelism-flaky harness tests** (issue #334): the CLI subprocess tests are serialized behind an async mutex and the `nproc` task-count test takes the best of a few attempts, so they pass at default parallelism, not only under `--test-threads=1`.

## [v2026.6.15-0] — 2026-06-15 — 2.31.0

The **subscription-CLI backend + Phase 6 (paññā)** release: agents can now run on a Claude/Codex subscription with no API key, the harness eval framework and sandbox hardening extend across backends and platforms, the deep-memory store stops being a secret sink, and an **experimental** computer-use scaffold lands behind a feature flag.

### Added

- **`cli` provider backend (#277)** — drive a local, subscription-authenticated vendor CLI (`claude`, `codex`, …) instead of an HTTP endpoint: `bwoc-harness --backend cli [--cli-cmd claude]` shells out per turn (`<cli> -p --model <model> --output-format json`, conversation on stdin), so agents run on a Claude/Codex **subscription with zero API key**. Chat-only by design (the vendor CLI executes its own tools internally; harness tool-calling stays on HTTP backends). New manifest field `cliCmd` (optional, defaults `claude`).
- **macOS network-egress parity in the sandbox SBPL (t29, #283)** — the seccomp network-egress containment from Phase 5 now has a macOS Seatbelt (SBPL) equivalent, so the no-egress invariant holds on macOS as well as Linux.
- **`cli` ambient-backend trust tier (t30, #284)** — the `cli` backend is host-credentialed (ambient), so it is refused for untrusted auto-process: an unverified inbound message can never drive a subscription CLI.
- **Eval skips tool-fixtures on an ambient backend (t31b, #286)** — the eval harness recognizes ambient (chat-only) backends and skips tool-call fixtures that cannot apply to them, instead of scoring them as failures.
- **Deep-memory `mine` secret redaction (#289)** — credentials (API keys, bearer/Slack/GitHub tokens, JWTs, PEM private keys, `key=value` secrets) are scrubbed from mined session text **before** embedding and storage, so the deep-memory store never becomes a secret sink. `mine` reports the redaction count. Pure-Rust (`regex`), quarantined to `bwoc-deep-memory`.
- **Computer-use scaffold — experimental (#291, #292)** — a backend-neutral `ComputerAction` model + Anthropic native-tool serialization (`computer_20250124` + beta header), and a headless-browser `ComputerExecutor` (CDP via `chromiumoxide`) behind the optional **`browser`** feature. **Experimental:** not wired into the agent loop; the default build pulls zero browser deps; security gating (screenshot taint + capability gate + autoprocess refusal) is still ahead.

### Changed

- **`agent_loop.rs` decomposed into a directory module (t31a, #285)** — internal structure only; no behavior change.
- **Auto-version hook bumps shared version lines only on `main`, not feature branches (#287)** — feature branches no longer touch `Cargo.toml`/`VERSION.md` shared lines, so concurrent PRs never collide on the version.
- **Fork-heavy integration tests serialized in CI (t17, #282)** — avoids resource-contention flakiness in the sandbox/process tests.
- **`VERSION.md` / README / ROADMAP mark Phase 6 — _paññā_ (#290)** — harness eval & cross-platform hardening; t29–t31 done, t32 (deep-memory sqlite-vec / governance) parked as premature.

### Fixed

- **`bwoc-connect` now forwards the agent's `backend` (and `cliCmd`) to the spawned harness** — connect sessions previously ignored the manifest backend and always ran on the harness default, so `openrouter`/`claude`/`cli` agents silently used the wrong provider (#277).
- **`bwoc okr track` no longer hangs on idle stdin nor drops `as_of`/`evidence` (#280)** — the track verb blocked on stdin even when all fields were supplied as flags (hanging non-interactively) and silently discarded `as_of`/`evidence`; both fixed.

## [v2026.6.9-2] — 2026-06-09 — 2.30.0

The **standalone agent** release: an incarnated agent now runs as a self-hosted, gateway-reachable unit — receiving signed envelopes from peers across NAT/the internet, vetting them through the trust gate, auto-processing untrusted ones in a read-only turn, and shipping as a container image.

### Added

- **Standalone agent gateway receive bridge** — `bwoc-agent --serve` supervises a `bwoc-gateway-recv` child that maintains the `wss://` relay connection and drops inbound signed envelopes into the agent's inbox, so an agent behind NAT stays reachable without a direct listener. Network/TLS deps stay confined to the gateway binaries (dep-quarantine holds — `bwoc-agent`/`bwoc-core` never link WS/TLS). (#269)
- **Pinned-peer keyring + replay defense** — remote senders are resolved against a pinned `.bwoc/peers.toml` keyring; the Kalyāṇamitta-7 trust gate verifies the signature, resolves the sender, and rejects replayed envelopes, so an unverifiable or replayed message from across a gateway is refused rather than delivered. (#270)
- **Untrusted gateway auto-process + reply** — inbound envelopes from a gateway can be auto-processed in an untrusted, read-only turn (PureRead tools only) and replied to over the same transport, closing the loop for an unattended standalone agent while keeping the untrusted-ingress surface read-only. (#271)
- **Standalone agent container image** — `deploy/standalone-agent.Dockerfile` builds a self-contained image (all five binaries on `PATH`, non-root `agent` user, `ENTRYPOINT ["bwoc-agent","--serve"]`); the ed25519 identity key and provider creds are mounted at run, never baked into a layer. (#273)
- **Operating HANDBOOK (EN/TH)** for the agent template (`modules/agent-template/docs/{en,th}/HANDBOOK`) — a single onboarding read covering identity, memory, skills/mindsets, interconnect, backends, and verification gates. (#275)

### Fixed

- **t6 `RLIMIT_NPROC` floor counts tasks, not processes** (`bwoc-harness`) — the per-turn process-count limit was measured against processes rather than tasks (threads), which could trip the floor on legitimate multi-threaded turns. (#267)
- **What's New release-gate** (`bwoc-cli`) — a CI-enforced assertion now fails the build until a `HIGHLIGHTS` bullet cites the current `MAJOR.MINOR`, so "update What's New every release" is enforced rather than remembered. (#274)

## [v2026.6.9-1] — 2026-06-09 — 2.29.0

### Added

- **`openrouter` provider backend** for `bwoc-harness` — drive any vendor's model (OpenAI / Anthropic / Google / Meta / NVIDIA / …) through [OpenRouter](https://openrouter.ai)'s hosted OpenAI-compatible aggregator with one key. OpenRouter speaks the exact OpenAI shape the harness already implements, so the existing `OllamaClient` was extended rather than duplicated: optional bearer auth (`with_api_key`) + attribution headers (`with_headers`) applied at every request site; `api_key = None` keeps the plain-Ollama path byte-for-byte unchanged. The key resolves from `OPENROUTER_API_KEY` or `~/.bwoc/secrets.toml` `[openrouter] api_key` (chmod-600 guarded, sharing Anthropic's resolver), and a missing key fails fast with an actionable message instead of a bare `HTTP 401`. Agent manifest: `"backend": "openrouter"`, optional `"baseUrl"` (defaults to `https://openrouter.ai/api/v1`). Wired through `bwoc-cli` (`spawn`/`run`/`chat`) and the chat TUI (`harness_argv` now forwards `--backend`, which is load-bearing for `openrouter` — without it requests fall through to the unauthenticated client). See `notes/2026-06-09_openrouter-provider-backend.md`. (#268)

## [v2026.6.9-0] — 2026-06-09 — 2.28.0

### Added

- **`RouteTarget::Gateway` transport** in `routes.toml` — a third delivery transport alongside `local` and `mqtt`, for peers with no direct reachability (across NAT/firewalls/the open internet) reached through a `bwoc-gateway` relay. A route declares `transport = "gateway"` + `gateway = "wss://…/v1/connect"`; `bwoc send` resolves it and shells out to the `bwoc-gateway-send` sibling binary (dep-quarantine: `bwoc-core`/`bwoc-cli` never link a WebSocket/TLS client), piping the signed message envelope over stdin exactly as the MQTT path does. The sender's keypair is the gateway login, so the transport requires a signed agent sender (`user`/unsigned origins error with guidance). `bwoc peer` lists gateway routes. See `notes/2026-06-09_route-target-gateway.md`.

### Fixed

- **Flaky `process_isolation` test suite** (`bwoc-harness`) on `ubuntu-latest` CI — a different subset of its 12 sandbox tests failed each run. They each spawn the real turn-executor child and contend on process-wide machinery (capability token, per-turn `setrlimit`/cgroup, IPC fd, env scrub, PID reaping) when run in parallel. Serialized the suite with a dependency-free, poison-tolerant file-level lock. See `notes/2026-06-09_process-isolation-serial.md`.

## [v2026.6.8-0] — 2026-06-09 — 2.27.0

### Added

- **Phase 5 — *saṃvara* (trust-boundary & sandbox hardening).** Closes the untrusted-ingress surface that Phase 3's chat-connectors opened into the self-hosted `bwoc-harness`. The full 8-gate DoD plus the network-egress hard-blocker, each plan→Pavāraṇā→implement with adversarial red-team verification:
  - **t1 — total ingress trust-labeling.** Every `ChatMessage` carries an immutable `Principal`; `TrustLevel` is *derived*, never stored (promote-to-trusted is unrepresentable); fail-closed. Fixes a teammate `role:System` laundering bug.
  - **t2 / t3 — Layer-0 capability gate + taint propagation.** Untrusted turns are read-only by default (zero allow-by-omission); a sticky `untrusted_seen` latch survives compaction + reload; the graded gate allows worktree-confined writes while gating escape/persist/destruct.
  - **t4 — egress-clean whitelist proof.** `PURE_READ_TOOLS` proven egress-clean by a fail-closed tripwire + static scan + a CI guard that makes allow-by-omission impossible.
  - **t5 — per-turn process isolation.** Effectful tool execution runs in a re-exec'd, single-use turn-executor child with an unforgeable one-time token, env-scrub, and fd hygiene; un-marshallable tools fail closed.
  - **t6 — per-turn `setrlimit`** (CPU / AS / NOFILE / FSIZE; relative best-effort NPROC).
  - **t7a — executor FS jail** (Landlock / sandbox-exec) + `PR_SET_DUMPABLE` + yama (anti-ptrace) + jailed build/`core.hooksPath` (closes the `build.rs` RCE).
  - **t8 — deferred-control fence**: an honest residual table + a CI `fence-guard` that fails if code assumes an un-shipped control.
  - **t9 — per-turn cgroup v2 `pids.max`** (best-effort-when-delegated; CI-proven fork-bomb containment).
  - **t11 — seccomp-bpf network-egress + ptrace hardening** (the no-fd invariant: arch-guard KILL, `close_range`, `pidfd_getfd`/`sendmmsg` denies, fail-closed on Linux).
  - Linux-first; macOS degrades with loud skips, never silent-pass. Local same-uid covert channels are explicitly out of scope (network-egress containment, Linux). See `docs/en/THREAT-MODEL.en.md` fence + `notes/2026-06-08_phase5-samvara.md`.

### Fixed

- **Windows build + cfg-gating** of the unix-only sandbox/jail/seccomp/cgroup paths; **`c2_token_scrubbed_before_grandchild`** IPC-read flake stabilized.

## [v2026.6.7-1] — 2026-06-07 — 2.26.0

### Added

- **Chat connectors — LINE (`bwoc-connect line`).** Bridges LINE to a BWOC agent over the Messaging API. Unlike Telegram/Discord, LINE **pushes to an HTTPS webhook** — so the transport runs a small inbound **axum** server: each POST is signature-verified (`X-Line-Signature` = base64(HMAC-SHA256(channel secret, body))), parsed, and queued; `poll` drains the queue (the Discord gateway→mpsc pattern). **Free for our use case**: replies use the one-time `replyToken` (free, unlimited) when fresh, falling back to a **push** (monthly quota) for a slow turn. LINE has no edit API → non-streaming (`supports_edit = false`; the bridge sends the reply once). LINE's string ids (`U…`/`C…`/`R…`) are hashed to a stable `i64` so they fit the existing allow-list seam; the allow-list is configured as LINE user ids under `[line].allow_user_ids` (closed by default). Token from `LINE_CHANNEL_ACCESS_TOKEN` (keyring/env), secret from `LINE_CHANNEL_SECRET`; `bwoc-agent --serve` supervises `connectors/line.toml`. Dep-quarantine holds (axum/hmac/sha2/base64 in `bwoc-connect` only — verified absent from cli/agent/core). `verify_signature`/`parse_webhook`/`hash_id` + the non-editing single-send path are unit-tested; the live webhook/send are the eyeball-reviewed edge. Runs on Linux — unlike iMessage.

- **Chat connectors — reply streaming (in-place edits).** Telegram + Discord replies now stream: the bridge **sends** a message on the first token and **edits it in place** as the agent's reply grows, debounced to 1 edit/sec (clear of Telegram's ~1/s and Discord's ~5/5s limits), with a guaranteed final edit showing the complete text. `Transport::send` now returns the message id and gains `edit`; `AgentSession::ask_streamed` relays `chat_proto` `Token` deltas (the default still does a single send, so non-streaming sessions are unchanged). A new `PlatformStream` carries the send-then-debounced-edit logic, unit-tested (placeholder→edits, no-tokens→single-send, blank-skip, end-to-end via the bridge). No new deps.


## [v2026.6.7-0] — 2026-06-07 — 2.25.0

### Added

- **Chat connectors — keyring token resolution (the architect's "keyring default").** `bwoc-connect` resolves the bot token from the **OS keyring first on macOS/Windows** (service `bwoc/<platform>`, account = the agent dir's basename), falling back to the platform env var (`TELEGRAM_BOT_TOKEN` / `DISCORD_BOT_TOKEN`). **Linux is env-only** by design — wiring Secret Service pulls system libdbus (absent on CI) or a second async runtime, heavy for a feature the headless deployment target doesn't use (no Secret Service daemon → env anyway). The env var is the fallback on every platform; a missing/locked keyring is never fatal.

- **Chat connectors — `bwoc status` connector health.** `bwoc-agent --serve` now writes a `.bwoc/connector.status` marker (platform · state `running`/`exited`/`stopped` · pid) as it supervises a connector, and `bwoc status` surfaces it in a new **Connectors** section (e.g. `agent-pi  telegram  running (pid 1234)`). Fleet-visible connector health without disturbing the agent table; best-effort, no new dep (serde_json marker).

- **Chat connectors — hardening (post-PR4 review follow-ups).** Discord gateway: the first heartbeat now waits ~half the interval (`interval_at`) instead of firing immediately after IDENTIFY (off-spec), and `createMessage` failures include Discord's JSON error body. Group peer context is now tagged platform-aware (`dc:<id>` / `tg:<id>`) via `GroupBridge.peer_prefix` rather than a hard-coded `tg:`. `futures-util` enables `std` (it uses `StreamExt`), and the `NoToken` message no longer references the obsolete PR3. Remaining connector follow-ups: keyring token resolution, `bwoc status` connector health, gateway RESUME.

- **Chat connectors — Discord (PR4), completing the Telegram/Discord subsystem.** `bwoc-connect discord --agent <dir>` bridges Discord to a BWOC agent over the **Gateway websocket** (HELLO → IDENTIFY → heartbeat → `MESSAGE_CREATE` dispatch; intents `GUILD_MESSAGES|DIRECT_MESSAGES|MESSAGE_CONTENT`) with REST sends, reconnecting on drop. The routing/allow-list/group-vs-DM logic (`run_bridge`) is **reused unchanged** — only a new `DiscordTransport` (a background gateway task feeding a queue that `poll` drains) and the pure `parse_message_create` are Discord-specific. DMs vs guild rooms split on `guild_id`; mention-gating uses Discord's structured `mentions[]`; bot authors are skipped (no loops). Token from `DISCORD_BOT_TOKEN`; `bwoc-agent --serve` now supervises `connectors/discord.toml` too. Dep-quarantine holds: `tokio-tungstenite` lives only in `bwoc-connect` (verified absent from cli/agent/core). `parse_message_create` is unit-tested (DM / guild±mention / bot-author+empty skip); the gateway loop is the integration-untested live edge (no Discord token in CI). Telegram + Discord, DM + group, now both daemon-managed.

- **Chat connectors — daemon supervision (PR3).** `bwoc-agent --serve` now supervises an agent's connector: when `connectors/telegram.toml` has `enabled = true`, the daemon spawns `bwoc-connect telegram --agent <dir>`, respawns it on exit (5s backoff so a crash-loop can't spin hot), and kills it on shutdown — the same subprocess-supervision pattern as `bwoc-harness`, so the **dep-quarantine holds** (bwoc-agent gains only lean `toml`; reqwest/tokio stay inside `bwoc-connect`, verified). Detection + the enabled-flag read are unit-tested (enabled / disabled / absent / malformed). Keyring token resolution + `bwoc status` connector health are the remaining PR3 follow-ups; Discord is PR4.

- **Chat connectors — Telegram group ⇄ Saṅgha team chat (PR2).** Group/supergroup rooms now bridge to a team's shared `chat.jsonl` (HV3-3a). A `[group] team = "<id>"` binding in `telegram.toml` routes group messages from allow-listed senders: a message that **@mentions the bot** (or any message when `mention_only = false`) is served by a `--team-chat` agent session — which injects the room's recent peer messages and broadcasts its reply — and the reply goes back to the room; a **non-mention** message is appended to `chat.jsonl` as peer context (`from = "tg:<user>"`) so the agent has the conversation when next addressed. A group message with no team binding is ignored; the allow-list still gates who may reach the agent. The bot's `@username` is resolved via `getMe` at startup for mention-gating. 12 unit tests (DM/group classification, mention detection, mention→reply, non-mention→peer-log-no-reply, no-binding→ignored) behind the same `Transport`/`AgentSession` seams. Next: daemon supervision + keyring (PR3), Discord (PR4).

- **Chat connectors — `bwoc-connect`, Telegram DM (PR1 of the connector subsystem).** A new **dep-quarantined** crate (`reqwest`/`tokio` live here; `bwoc-cli`/`agent`/`core` trees stay clean — verified) that bridges external chat platforms to BWOC agents. `bwoc-connect telegram --agent <dir>` long-polls Telegram and, for each **allow-listed** sender, holds a `bwoc-harness --chat` subprocess and relays text both ways over the existing `bwoc_core::chat_proto` — so streaming/permission/compaction come free, no protocol invention. **Security-first**: a *closed-by-default* sender allow-list (empty ⇒ nobody — no public bots); the bridged session is non-TTY so `ask`-mode tools fail safe to **deny**, and a `PermissionRequest` is auto-denied (a remote user can never approve a tool call). The routing/allow-list/offset core (`run_bridge`) and the Telegram update parser are unit-tested behind `Transport`/`AgentSession` seams (no live bot needed). Token from `TELEGRAM_BOT_TOKEN` (the headless-server path); keyring resolution + daemon supervision + group rooms (HV3-3a `chat.jsonl`) + Discord follow in PR2–PR4 (design: `notes/2026-06-06_chat-connectors-design.md`).

- **`bwoc report` — file a GitHub issue from the CLI.** `bwoc report "<title>" [--body …] [--kind bug|feature|question] [--web] [--yes]` previews the issue and, after an explicit confirmation on an interactive terminal, creates it via `gh issue create` against the framework repo. **Fail-safe by construction**: unattended sessions (non-TTY without `--yes`), `--web`, or a missing/unauthenticated `gh` all fall back to printing a prefilled `issues/new` URL — a public issue is never filed unattended. The body always carries an Environment block (bwoc version, release identity, OS/arch) so reports are diagnosable in one round; `--kind feature` maps to the stock `enhancement` label. Shells out through the same `ShellRunner` seam as `bwoc update` (no new HTTP dependency; fully unit-tested with a mock, including the RFC 3986 percent-encoder for the query-string values).

- **GitHub Copilot CLI is the 6th backend (`copilot`) — Samānattatā.** `bwoc spawn`/`bwoc chat` exec the `copilot` agentic CLI in the agent's directory (it reads `AGENTS.md` natively, so the backend-neutral source of truth works unmodified; a `COPILOT.md → AGENTS.md` symlink joins the template set for convention). **`bwoc run` works headless too** — `copilot -p "<task>" --no-ask-user` (Copilot's programmatic mode; the permissive `--allow-all-tools` is deliberately not passed, matching GitHub's container-only guidance and our fail-safe posture) — making Copilot the second vendor backend with non-interactive support after Claude, a head start on HV3-6. Wired through `parse_backend` (chat/run), the `bwoc new` model picker (multi-model menu: Claude + GPT slugs), `bwoc doctor`'s PATH probe, `bwoc help backends`, the handbook (EN/TH), and `WORKSPACE` docs (EN/TH). No `reasoningEffort` flag (Copilot exposes none).

- **HV3-3c — peer-review gate (Kalyāṇamitta).** Completes HV3-3: a Saṅgha lead can route a successful worker's diff to a designated reviewer agent before completing the task. `bwoc-core::team::Team` gains a `reviewer` field (decided shape: **fixed reviewer per team**); new `bwoc-harness::review` defines a `Reviewer` trait + `ReviewVerdict` + a pure `parse_verdict` + `SubprocessReviewer` (spawns a `bwoc-harness` in the worker's worktree to inspect the diff, reads the verdict from the HV3-3b result envelope). The lead's success path now gates on it: **APPROVE** → complete + tear down; **REJECT** → unclaim (re-queue) keeping the worktree + feedback for the next claimer, counted in a new `LeadSummary.rejected`. Wired via `bwoc-harness --lead --reviewer <agent>`; a reviewer equal to the claiming agent (self-review) is skipped, and the gate is **fail-safe** — a reviewer spawn/timeout/unparseable-verdict resolves to REJECT, so unreviewed work never auto-completes (Sīla). CLI resolution of the team's `reviewer` field into `--reviewer` is a follow-up slice.
- **`bwoc handbook` — bundled offline quick guide.** A new command rendering a terminal-sized, task-oriented guide straight from the binary (no network, no files to locate). `bwoc handbook` lists sections; `bwoc handbook <section>` prints one — **start · agents · spawn · teams · harness · release**. Bilingual: the resolved language (`--lang` / `BWOC_LANG` / `LANG`) selects the Thai body, falling back to English. Content is purpose-written for getting moving; the full reference stays in `docs/`.
- **`bwoc info` — one-card system status.** Version + release identity (CalVer when a release build) + phase + workspace path + registered-agent count + update-drift status, in one card (`--json` for the machine-readable form). The update line is **read-only and offline** — it reuses the throttle cache the background version-check already maintains (no extra network), surfacing explicitly what bare `bwoc` already prints as a drift notice. New `update::info_status_line()` exposes that cached status.

## [v2026.6.6-0] — 2026-06-06 — 2.24.0

### Added

- **HV3-3a — team chat broadcast (Kalyāṇamitta).** Agents in a Saṅgha team can now see each other's chat replies through a shared append-only log. `bwoc-core::team` gains `TeamChatMessage` + `parse_chat`/`render_chat` — one JSON object per line in `.bwoc/teams/<team-id>/chat.jsonl`, the same storage model as the task list (no pub/sub, no broker). A `bwoc-harness --chat --team-chat <path>` session opts in: teammate messages posted since the last turn are injected as a "Team conversation (Saṅgha)" system note before each user turn, and the agent's reply is appended to the log afterward (append-mode, so concurrent teammates never clobber each other). Strictly opt-in and best-effort: no `--team-chat` keeps the session solo (zero behaviour change), a missing/unparseable log injects nothing, an agent never sees its own messages echoed back, and the cursor resyncs if the log is rotated/rewritten shorter (append is a single `O_APPEND write_all`). Reachable from the CLI as **`bwoc chat <agent> --tui --team <id>`**: it validates the agent's team membership, resolves `.bwoc/teams/<id>/chat.jsonl`, and forwards it to the harness (warn-and-run-solo if `--team` is given without a harness-backed `--tui` session, since vendor CLIs speak their own protocol). Teammate messages are also surfaced to the frontend: the harness emits a new `ChatEvent::TeamMessage { from, text, ts }` (additive, forward-compatible) for each previously-unseen peer message, and the `--tui` client renders it distinctly (`📢 <from>: <text>`) so the human follows the team thread, not only the agent's replies.

- **HV3-3b — worker result envelope (Kalyāṇamitta).** A Saṅgha worker now leaves a structured outcome in its worktree instead of only an exit code: at session end `bwoc-harness` writes `.bwoc/worker-result.json` (`bwoc_harness::result::WorkerResult` — task, success, turns, compactions, token-pressure switches, active model, a bounded summary, and a `DiffSummary` of working-tree changes vs the base `HEAD`, untracked files included). The lead reads it before tearing the worktree down and logs a one-line summary (`N turn(s), +X/-Y across F file(s), model=…`). Best-effort and backward-compatible: a worker that writes no envelope degrades silently to the exit code the lead already has. This is the seam HV3-3c's peer-review gate will tap — the reviewer reads the same envelope before gates run.

- **HV3-2 — unified context engine.** Batch and `--chat` now share one compaction policy (`compact::compact_context`): **summarize-first** (the oldest span folds into one LLM summary note) with a **truncate-with-marker fallback** when the summarizer fails — so the batch loop gains real summarization (it was truncate-only by v1 design) and chat gains the guaranteed-shrink fallback (a failing summarizer no longer leaves the history over budget), with no new failure mode in either path. **Tier 2 synergy:** what falls out of the window falls into memory — when `deepMemoryCmd` is configured the folded content (summary, or raw excerpt on fallback) is written to `.bwoc/compacted-context.md` and mined (`--mode compaction`), recallable later via `memory_search`. The loop's `compactions` metric now counts only passes that actually folded messages (previously a triggered no-op also incremented).

- **Harness v3 begins — HV3-1 "Memory in the loop" (Sati).** When the agent's manifest configures `deepMemoryCmd`, `bwoc-harness` closes the Tier 2 memory loop around every session: **wake-up** output joins the system prompt at session start (batch *and* `--chat`) as a "Prior context (Tier 2 memory)" block; a read-only **`memory_search`** tool (`<cmd> search`) lets the model recall past decisions mid-run through the normal guardrails → permission pipeline (chat's default policy allows it beside `memory_read`); and at session end the session is **mined** back into memory — chat mines its persisted `.bwoc/chat-session.json`, a successful batch run distils *task → outcome* into `.bwoc/last-run.md` and mines that (the checkpoint is cleaned up on success), and a failed run mines its surviving checkpoint. Strictly opt-in and best-effort: absent/placeholder `deepMemoryCmd` disables everything, failures degrade to warnings, and every call is timeout-bounded (10/15/60 s) so a hung backend can never stall a run. Verified live end-to-end: run 1 mined its distillate, run 2 woke with run 1's memory injected.

## [v2026.6.5-0] — 2026-06-05 — 2.23.0

### Added

- **Windows named-pipe daemon — `bwoc-agent --serve` now runs on Windows.** Replaces the exit-2 stub with a real daemon over a named pipe at `\\.\pipe\bwoc-agent-<hash>`, where the hash derives deterministically from the agent directory (`bwoc-core::ipc::pipe_name`, dependency-free FNV-1a) so server and clients meet without a rendezvous; the name is also recorded in `.bwoc/agent.pipe` for humans and `doctor`. The daemon body (`serve_core`) is now transport-independent — PID file, inbox watch + cursor, trust gating, Saṅgha task watch, and the line-text protocol (`PING`/`STATUS`/`STOP`) are one shared implementation; Unix keeps its exact `agent.sock` contract (still `nc -U`-debuggable). Clients gained Windows paths: `bwoc ping` / `status` (uptime) / `stop` speak the pipe; process liveness and the stop escalation use `tasklist` / `taskkill` (polite, then `/F`) — no new always-on deps (`interprocess` is `cfg(windows)`-only; dep-quarantine intact, `bwoc-core` untouched). A named-pipe protocol roundtrip test runs on the windows-latest CI leg. Closes the last code item under Phase 2 "Remaining for ship".

### Fixed

- **`bwoc new` now honours the standard workspace resolution for its default target.** Previously only the ancestor walk from *cwd* ran, so `BWOC_WORKSPACE=/scratch/ws bwoc new …` silently created and registered the agent in whatever workspace enclosed the current directory — e.g. the live fleet instead of the scratch one. The default target now resolves per WORKSPACE.en.md precedence: the new `--workspace <path>` flag > `BWOC_WORKSPACE` env > ancestor walk. An explicit `--target` still wins outright.

## [v2026.6.4-0] — 2026-06-04 — 2.22.0

### Added

- **Design system — `bwoc-core::design` tokens.** A single source of truth for the colours, glyphs, and spacing BWOC's three UIs render with (`bwoc dashboard`, `bwoc chat --tui`, the `bwoc-chat` desktop app), replacing per-UI hardcoded palettes that had drifted. Tokens are **plain data** (no ratatui/egui types — dep-quarantine intact): each `ColorToken` carries an `ansi` half (terminal UIs map it to *named* colours so the user's terminal theme keeps authority) and an `rgb` half (pixel UIs use it directly); glyphs are shared `&str`; spacing/typography are plain `f32` (`MESSAGE_GAP`, `LINE_HEIGHT_FACTOR` 1.4 for stacked Thai marks). Principles enforced by unit tests: activity glyphs pairwise distinct (state never reads by colour alone), selection hue ≠ idle/title hue (one meaning per colour per screen), muted floors at `Gray` (never `DarkGray`-on-dark). **Both framework TUIs now consume the tokens** — `bwoc dashboard` (incl. review fixes: selection moves from yellow to blue/white so yellow no longer means title+selection+idle at once; the navigable agents pane gets the accent border; muted text lifted from `DarkGray` to `Gray`) and `bwoc chat --tui` (status bar, borders, permission/outcome colours). Spec: `docs/en/DESIGN.en.md` (+ TH). The `bwoc-chat` desktop app (separate repo) is a follow-up.
- **`bwoc remote` — link agents to remote-control sessions and manage them.** A *remote-control session* lets an agent's interactive session be driven from elsewhere (e.g. Claude Code's Remote Control, reachable from claude.ai / mobile); `bwoc remote` is **backend-neutral bookkeeping** over that relationship — it records which external control session each agent is linked to, without opening or proxying the session itself. `bwoc remote link <agent> <session-ref>` writes a record at `.bwoc/remote/<agentId>.json` (`{ agentId, backend, kind, sessionRef, url?, linkedAt, note? }`, mirroring the `.bwoc/sessions/` marker convention); `--backend` defaults from the workspace registry (`agents.toml`) and `--kind` defaults to `claude-remote-control` (the first mechanism — any backend may declare its own `kind`, so Claude is the first implementation, not a special case). `bwoc remote list` / `status <agent>` read (`--json`; `list` flags orphaned links whose agent is no longer registered); `bwoc remote unlink <agent>` is a gated remove (TTY confirm unless `--yes`).

### Fixed

- **`bwoc dashboard` banner no longer clips its attention indicator.** The workspace line renders un-wrapped, so a long workspace path pushed the rightmost — most important — parts (`attention: N pending`, the counts) off-screen. The path now elides its *middle* against the pane width (the tail keeps the directory name); the counts and attention always render.
- **`bwoc dashboard` TUI UX (P1s from the design review).** (1) The footer hotkey legend — one un-wrappable row — clipped its rightmost hints (incl. `q/Esc`) on narrow terminals, worse with longer i18n labels (e.g. Thai); below 100 cols it now falls back to a core legend (`↑↓ · t · ? · q`) with `?` carrying the full list. (2) The `?` help overlay (fixed 60% height) clipped its bottom lines on short terminals; it is now sized to its content (+ wrapped), centred and clamped. (3) Below 60×16 the bordered panes collapsed into garbage; a centred "terminal too small" hint renders instead. (4) a11y: `working` and `running` no longer share the `●` glyph — `working` is `◉`, so activity states read without relying on colour.

## [v2026.6.3-1] — 2026-06-03 — 2.21.0

### Added

- **New `bwoc-deep-memory` crate — the Tier 2 deep-memory reference implementation.** A self-contained binary that speaks the backend-neutral `bwoc-core::deep_memory` contract (`wake-up` | `search "<q>"` | `mine <path> --mode <m>`) over a local SQLite store with **semantic (embedding) recall**, so a fresh `bwoc new --deep-memory-cmd bwoc-deep-memory …` works out of the box instead of pointing at a tool the operator must supply. `mine` walks session files (`md/txt/jsonl/json/log`, 5 MiB/file cap), chunks at paragraph boundaries, embeds via any OpenAI-compatible `POST /v1/embeddings` endpoint (Ollama/llama.cpp/vLLM/OpenAI), and stores `f32` vectors as BLOBs; `search` ranks by **brute-force cosine** in Rust (no native-extension build risk — a `sqlite-vec` k-NN backend can swap in later behind the unchanged `Store` seam); `wake-up` emits the most-recent memories for session-start injection. The `Embedder` trait is injectable — a deterministic `StubEmbedder` keeps the verb logic unit-tested offline, mirroring the existing `RunnerFn` seam. Dep-quarantine preserved: `rusqlite` (`bundled`) + `reqwest` live only in this crate, never `bwoc-core`. **Closes the last item deferred off the Phase 3 DoD** (the interface — `bwoc-core::deep_memory` + the `bwoc memory wake-up|search|mine` CLI surface — already shipped; this is the reference backend it dispatches to). Config: `--db`/`--embed-url`/`--embed-model` flags > `BWOC_DEEP_MEMORY_DB`/`BWOC_EMBED_URL`/`BWOC_EMBED_MODEL`/`BWOC_EMBED_API_KEY` env > defaults.
- **`bwoc debase` — manage the agent → base-project binding as a first-class surface.** An agent's *base project* (the one it derives from and builds) is carried functionally by its manifest `worktreeBase`: an agent bound to project `P` has `worktreeBase = <P>/worktrees`, and the framework places task worktrees at `<worktreeBase>/<agentId>/<taskId>`. The new command makes that relationship inspectable and editable: `bwoc debase list` (every agent → base project + buildable stack, `--json`), `bwoc debase show <agent>` (one agent's binding detail incl. the worktree pattern), and `bwoc debase set <agent> <project>` (a gated write — TTY confirm unless `--yes` — that points the agent's `worktreeBase` at `<project>/worktrees`; idempotent, canonicalizes the path, refuses a non-existent project). Unbound agents (no `worktreeBase` — e.g. monitoring/audit agents that operate *on* targets) show as `—`.
- **`bwoc new --project <path>` — derive an agent already bound to a project.** Incarnates the agent and, in one step, sets `worktreeBase` to `<project>/worktrees` and seeds the build/test/lint/format gates from the project's detected stack (Rust/Node/Python/Go). Explicit gate flags (`--lint-cmd` etc.) and `--worktree-base` always win over the derived defaults. Pairs with `bwoc debase` for the lifecycle of the binding.

## [v2026.6.3-0] — 2026-06-03 — 2.20.1

### Added

- **Cross-backend validation workflow (`.github/workflows/cross-backend.yml`).** Proves Samānattatā by running one agent profile through the full uppāda → ṭhiti arc (`bwoc init` → `bwoc new` → `bwoc check` → `bwoc run`) on the **ollama** backend — the one needing no API key (installs Ollama + a tiny model, `qwen2.5:0.5b`, in the runner). Runs on push-to-main + nightly + manual dispatch — not on every PR (the fast gate in `ci.yml` stays the PR gate), since model pulls are slow. Closes the "Cross-backend validation" item under Phase 2 "Remaining for ship" for the ollama backend; the four vendor backends (claude / codex / kimi / antigravity) are a documented follow-up, gated on operator-provisioned API-key secrets.
- **Cross-backend validation now also covers the `claude` backend** (it runs through `bwoc-harness` like ollama). A second job in `cross-backend.yml` runs the same uppāda → ṭhiti arc with `--backend claude` (Sonnet), gated on an `ANTHROPIC_API_KEY` secret — the job no-ops with a warning when the secret is absent (free until an operator provisions it), and the secret is referenced statically (no dynamic indexing) to satisfy CodeQL. The remaining vendor backends (codex / kimi / antigravity) are vendor-CLI execs, not harness/API-key, so their CI coverage needs the vendor CLI installed + authenticated — a separate follow-up.

### Security

- **`bwoc init` now gitignores the Trust v2 agent signing private key.** The `.gitignore` template gained `agents/*/.bwoc/agent.key` — the ed25519 **private** key `bwoc trust --keygen` writes (mode `0600` on Unix; non-Unix sets no perm restriction). User workspaces track `agents/` (only daemon ephemerals were ignored), so without this pattern a `git add -A` after keygen would commit an agent's private identity key. The matching **public** key (manifest `trust.signingPublicKey`) stays tracked, as intended. The pattern lives in the shared tail, so it applies under `--no-runtime` too. (Sīla — Adinnādāna.)

## [v2026.6.1-1] — 2026-06-01 — 2.20.0

### Added

- **`bwoc send` publishes over MQTT for `transport = "mqtt"` routes.** When a recipient resolves to a `RouteTarget::Mqtt` route, `bwoc send` (and `bwoc peer`) now build + sign the envelope as usual and publish it via the `bwoc-mqtt` sibling binary (`bwoc-mqtt publish`) instead of writing a local inbox — completing cross-machine delivery: sender → broker → the peer's `bwoc-mqtt serve` → recipient `inbox.jsonl`. Local-FS routes are unchanged. Dep-quarantine preserved: `bwoc-cli` spawns `bwoc-mqtt` rather than linking an MQTT client.
- **New `bwoc-mqtt` crate — MQTT transport for inter-workspace routing.** `bwoc-mqtt publish` pushes one envelope (the same JSON line `bwoc send` writes) to a broker topic; `bwoc-mqtt serve` subscribes (default `bwoc/+/inbox`) and appends each received envelope to the matching agent's `inbox.jsonl`, resolved via the local `AgentsRegistry` — so MQTT delivery and local-FS delivery produce identical inbox lines. Pure helpers (broker parse, topic derivation, recipient extraction, inbox resolution) are unit-tested; the `rumqttc` I/O is verified end-to-end against a live broker. Dep-quarantined: only `bwoc-mqtt` links an MQTT client.
- **Routing tables gain an MQTT transport target.** A `[[route]]` in `.bwoc/interconnect/routes.toml` now declares `transport = "local"` (default — a peer `workspace` path, unchanged) or `transport = "mqtt"` (a `broker` URL + optional `topic`, defaulting to `bwoc/<recipient-id>/inbox`) for cross-machine federation. `bwoc-core` stays MQTT-dependency-free — routes carry plain strings; `Routes::resolve_target` surfaces the `RouteTarget` and the publish lives in the forthcoming `bwoc-mqtt` crate. Pre-MQTT routes (just `workspace`) keep their exact meaning.
- **Native Anthropic (Claude) provider in `bwoc-harness`.** A second `ProviderClient` (`AnthropicClient`) speaks the Anthropic Messages API (`POST /v1/messages`), translated into the same OpenAI-shaped `provider::types` so the chat/agent loops, tool dispatch, and streaming accumulators are unchanged. `claude`-backend agents now run through the harness and emit the `chat_proto` stream (so `bwoc-chat` can render a native window for them). A new `--backend` flag (default `ollama`) selects the provider via a `build_provider()` factory; for `claude`/`anthropic` with the endpoint left at the Ollama default, it substitutes `https://api.anthropic.com`. Auth via `ANTHROPIC_API_KEY`, falling back to `~/.bwoc/secrets.toml` (`[anthropic] api_key`, 0600) so a GUI-launched `bwoc-chat` works without an exported env. Translation covers request mapping (system lift, `tool_use`, merged `tool_result` turns, `input_schema`), response parsing, and typed SSE streaming with usage stitching.
- **Automatic context compaction in `--chat`.** When the conversation's estimated size crosses the session's `max_context_tokens` budget, the oldest turns are summarized (one provider call) into a single note before the next request, keeping the recent tail verbatim — so a long chat stays under the model's window without losing earlier decisions. The harness emits a `chat_proto::ChatEvent::Compacted { removed }` notice; a summarizer failure is non-fatal (the turn proceeds uncompacted). The split never starts the kept tail on an orphan tool result. Adapted from openclaude's `autoCompact`.
- **Plan mode in `--chat`.** A fourth permission mode (`SetMode "plan"`): only read-only tools (`read_file` / `list_dir` / `grep` / `memory_read`) run; **every other tool** — writes, shell, `git`, task/peer/run delegation, gates, memory writes, and any future tool — is refused before the permission gate (allow-list, not deny-list) with a note telling the model to present its plan instead of acting. Switch back to `default` / `accept_edits` / `bypass` to execute. Adapted from openclaude's plan mode; reuses the existing `SetMode` / `ModeChanged` wire.
- **Live permission modes in `--chat`.** A new `chat_proto::ChatInput::SetMode` switches the session's permission posture without restarting: `default` (prompt for every `ask`-mode tool), `accept_edits` (auto-approve file write/edit tools, still prompt for the rest), or `bypass` (auto-approve every `ask`-mode tool). The harness acks with `ChatEvent::ModeChanged`. Hard `deny` rules and guardrails are never relaxed — modes only turn `ask` into auto-allow. Adapted from openclaude's permission modes; additive + backward-compatible.
- **`bwoc-harness --unrestricted` lifts the workdir file-access sandbox.** With the flag, the file tools (`read_file`/`write_file`/`edit_file`/`list_dir`/`grep`) may touch **any absolute path** on the machine; relative paths still resolve against `--workdir`. Without it, the path-traversal confinement is unchanged (the safe default). The safety gate shifts to the permission policy, so this is meant for an `ask`-gated / operator-reviewed session. `bwoc-chat` (desktop) passes `--unrestricted` so a chat agent can read & edit your real project files, with each write/edit surfaced as an Allow/Deny prompt.
- **`--chat` defaults to an `ask` permission policy** when the workdir has no `.bwoc/harness-policy.toml` (instead of the batch path's fail-safe deny): read-only tools run freely, while writes/edits/`run_command`/`git` prompt the frontend. An interactive client is always present in chat to answer, so file editing works out of the box without hand-writing a policy.
- **The `bwoc-harness --chat` session now remembers conversations.** The conversation is persisted to `<workdir>/.bwoc/chat-session.json` after each turn and reloaded on the next launch, so the **agent keeps full context across restarts** (not just a display transcript). On connect the prior turns are replayed to the frontend via a new `chat_proto::ChatEvent::Restored { role, text }`; a new `ChatInput::Forget` clears the memory (history + file). Both additions are backward-compatible (additive variants).
- **The `bwoc-harness --chat` driver now streams token deltas.** Each assistant turn is generated via the provider's streaming API and emitted as `chat_proto` `Token` events as the tokens arrive (followed by the final `Message`), so frontends — `bwoc chat --tui` and the `bwoc-chat` desktop app — render the reply live instead of all-at-once. Tool calls + the interactive permission round-trip are unchanged.
- **The `chat_proto` `Ready` event now carries the agent's tool names** (`tools: Vec<String>`, `#[serde(default)]` for backward compatibility), so a frontend can show what the agent can do (e.g. a `/tools` command).

### Changed

- **`edit_file` now falls back to whitespace-tolerant matching.** When an exact `old_string` match fails, the tool matches the file line-by-line ignoring each line's leading/trailing whitespace, and re-indents `new_string` to the file's actual indentation. This rescues the most common small-model edit failure — an `old_string` whose indentation is slightly off — while still refusing an ambiguous multi-site match (asks for more context instead of guessing). The result message reports which strategy matched (`exact` / `whitespace-tolerant`). Pattern adapted from openclaude's `FileEditTool`.

### Fixed

- **Restored: the chat driver emits `TurnEnd` on every error path.** A frontend treats `TurnEnd` as the per-turn "ready for next input" delimiter; the provider-error / empty-response paths had regressed to returning without it (a #160 review fix lost during the parallel-build integration). Re-added the `emit_turn_end` helper + calls alongside the streaming rewrite.

## [v2026.6.1-0] — 2026-06-01 — 2.19.0

**Minor release.** Agentic chat TUI + cross-agent delegation, with hardening and a crate split. Headline: `bwoc chat --tui` — a full-screen ratatui chat for the ollama / openai-compatible backends with interactive permission prompts — and the `bwoc_run` tool, letting a model launch another BWOC agent. Plus shell-operator-aware `run_command` guardrails, a provider request timeout, lead→worker budget/vetting propagation, and the audit-plugin environment scrub. Cargo SemVer `2.18.1` → `2.19.0`.

### Added

- **`bwoc chat <agent> --tui` — full-screen agentic chat TUI** for the ollama / openai-compatible backends. A ratatui client (status / conversation / tools-activity / input panes) drives a `bwoc-harness --chat` subprocess over a new JSON-line protocol (`bwoc_core::chat_proto`: `ChatEvent` out, `ChatInput` in) and renders streaming turns, tool calls, and interactive `[a]llow?`/`[d]eny` permission prompts. Vendor-CLI backends (claude / codex / kimi / agy) print a hint and fall back to the default exec path. The renderer lives in its own **`bwoc-tui`** crate (depends only on `bwoc-core` + ratatui/crossterm — never on `bwoc-harness` or `bwoc-cli`, preserving the dep-quarantine; the harness is a runtime subprocess, resolved as a sibling of the running `bwoc`).
- **`bwoc_run` harness tool — "ollama launches bwoc".** The model running a `bwoc-harness` loop (the ollama / openai-compatible backends) can now delegate a self-contained subtask to *another* BWOC agent by calling the `bwoc_run` tool, which shells out to `bwoc run <agent> --task <task> --json --timeout <n>` and returns the captured result. Like every tool it is **denied by default** (the permission policy's fail-safe `default_mode`), so it only fires when an operator opts in via `.bwoc/harness-policy.toml`; that same gate bounds recursion (a delegate can re-launch only if its own policy allows it), and each launch is time-bounded (`timeout_secs`, default 300).

### Changed

- **Lead mode forwards budget/vetting flags to its Saṅgha workers.** `bwoc-harness --lead` now propagates `--token-budget` / `--cost-limit` / `--cost-per-1m` / `--vetted-mode` to each spawned worker (previously silently dropped), so a budget or vetting policy set on the lead governs its workers too. The worker argv is built as `OsString` end-to-end so a non-UTF8 worktree path is passed verbatim.

### Fixed

- **`run_command` guardrails are shell-operator-aware.** The destruction / gate-bypass / privilege-escalation checks keyed off the first whitespace token, but the verbatim command runs via `sh -c`, so `true && rm -rf ~` slipped its destructive second segment past them. The command is now split on `;` `&&` `||` `|` and each segment gets the existing checks (an operator-level split, not a full POSIX parser — quoting / substitution remain the OS sandbox's job). The splitter compares ASCII operator bytes directly, so it can't panic on non-ASCII input.
- **The provider HTTP client now has a request timeout (120s).** A hung completion previously never resolved and bypassed the retry/backoff/budget path; it now surfaces as a transient error the existing retry loop handles.

### Security

- **Audit plugins no longer inherit the operator's environment.** `bwoc audit run` spawns each audit plugin — third-party code installed from a git/tarball URL — and the spawn carried the full ambient environment, leaking `GITHUB_TOKEN` / `AWS_*` / `NPM_TOKEN` etc. to exactly the process whose job is to run code you don't yet trust. The spawn now starts from a scrubbed environment (`env_clear()` + an allowlist filter) plus only the three `BWOC_*` context vars. The scrub logic + allowlist moved to a shared `bwoc_core::env_scrub` so the audit runner and the harness sandbox enforce the identical rule (the harness `sandbox::scrub_env` re-exports it; no new crate dependency — `bwoc-cli` already depends on `bwoc-core`, and the dep-quarantine on `bwoc-harness` is preserved).

## [v2026.5.31-4] — 2026-05-31 — 2.18.1

**Patch release.** Binary-resolution correctness: every place a BWOC binary launches another now runs the binary actually installed alongside it, not whatever stale copy is first on `$PATH` (dev builds, side-by-side version installs). Plus two internal de-duplications of drift-prone constants. Cargo SemVer `2.18.0` → `2.18.1`.

### Fixed

- **`bwoc chat --tmux` (and the dashboard's `t`/`g`/`l`/`i`/start/stop shell-outs) now re-invoke the *running* `bwoc` binary instead of a bare `bwoc` PATH lookup.** The tmux window / Ghostty window / captured child launched whatever `bwoc` was first on `$PATH`, so a dev build or a non-PATH install silently spawned a *different, stale* binary (e.g. a 2.18 build opening a 2.11 install) — or, with no `bwoc` on `$PATH` at all, the window flashed "command not found" and vanished. All these launchers now resolve `std::env::current_exe()` (new `spawn::bwoc_exe()` helper, mirroring `harness_binary`'s sibling-of-the-running-binary rule), falling back to `"bwoc"` only when `current_exe()` is unavailable.
- **Every BWOC binary that re-invokes a *sibling* binary now resolves it relative to the running executable, via the shared `bwoc_core::exec::{sibling_binary, binary_or_name}` helper.** Previously `bwoc start` / `bwoc supervise` spawned the daemon as a bare `bwoc-agent`, and the harness/agent shelled out to a bare `bwoc` (`task`, `send`, auto-claim) — each a `$PATH` lookup that could hit a different, stale version than the one running. The three-tier rule (sibling-of-`current_exe` → `CARGO_BIN_EXE_<name>` → `$PATH`, including Windows `<name>.exe` siblings) is now defined once in `bwoc-core` and reused; `spawn::harness_binary` was de-duplicated onto it.

### Changed

- **Single source of truth for two drift-prone constants.** The default Ollama endpoint (`http://localhost:11434/v1`) was hardcoded in four places across `bwoc-harness` — now one `provider::client::DEFAULT_ENDPOINT`. The `evidence.kind` enum (`EVIDENCE_KINDS`) was duplicated verbatim in `check.rs` and `audit.rs` — now one `pub(crate)` const in `check`. The startup banner's backend list also gained the missing `openai-compatible` so it matches the six backends documented elsewhere.

## [v2026.5.31-3] — 2026-05-31 — 2.18.0

**Minor release.** Self-hosted runtime observability + parallelism (`bwoc-harness`): per-turn `gen_ai.request.model` (#BWOC-11) and per-tool `execute_tool` OTel spans (#BWOC-13) under the opt-in `--features otel`, and `--lead` now actually runs up to `--concurrency` workers in parallel (#BWOC-14). Cargo SemVer `2.17.1` → `2.18.0`.

### Added

- **`bwoc-harness --lead` runs workers concurrently (`--concurrency`, BWOC-14).** The Saṅgha lead collected tasks one-at-a-time even with `--concurrency > 1`; now it keeps that many workers in flight (the queue's worker loop spawns each item under a `Semaphore`, the lead tops the queue up as workers finish). `--concurrency 1` is unchanged one-at-a-time behaviour; claim/complete bookkeeping stays single-threaded so there are no shared-state races.
- **`bwoc-harness` per-tool `execute_tool` OTel spans (BWOC-13).** Completing the BWOC-10 follow-up, each tool the model requests now emits an `execute_tool` child span (`gen_ai.operation.name=execute_tool`, `gen_ai.tool.name`) nested under its `bwoc.turn` span. `TurnMetrics` gains an additive `tool_names` list. Same env-gate + dep-quarantine; default build unchanged.

- **`bwoc-harness` per-turn model on OTel spans (BWOC-11).** Each `bwoc.turn` child span now carries `gen_ai.request.model` (the active model that turn, recorded in `TurnMetrics.model`) — useful when token pressure switches the model mid-session. `session-metrics.jsonl` gains an additive `model` field (omitted when empty). Default build + behaviour unchanged.


## [v2026.5.31-2] — 2026-05-31 — 2.17.1

**Patch release.** OpenTelemetry exporter modernization for `bwoc-harness` (opt-in `--features otel`; the default build is unchanged): bumped to opentelemetry 0.32, runtime env-gated, GenAI-semconv attributes, and per-turn `bwoc.turn` child spans (#140, #141). Cargo SemVer `2.17.0` → `2.17.1`.

### Changed

- **`bwoc-harness` OpenTelemetry exporter modernized (BWOC-2).** The optional `--features otel` OTLP exporter moved from opentelemetry 0.27 to **0.32**, is now **runtime env-gated** (silent no-op unless `OTEL_EXPORTER_OTLP_ENDPOINT` is set, so an `otel` build costs nothing until a collector exists), emits GenAI-semconv token usage (`gen_ai.operation.name`, `gen_ai.usage.input_tokens`/`output_tokens`) on the session span, and flushes via an explicit `provider.shutdown()` so a short-lived CLI doesn't drop the span on exit. The default build keeps **zero** OpenTelemetry dependencies (dep-quarantine). (Per-turn child spans land in the BWOC-10 entry below.)
- **`bwoc-harness` per-turn OTel spans (BWOC-10).** Building on BWOC-2, each recorded turn is now replayed as a `bwoc.turn` child span under the session span, carrying that turn's `gen_ai.usage.input_tokens`/`output_tokens`, tool-call count, and a duration reconstructed from `latency_ms`. Same env-gate + dep-quarantine as the session span (no-op unless `OTEL_EXPORTER_OTLP_ENDPOINT` is set; zero deps in the default build).

## [v2026.5.31-1] — 2026-05-31 — 2.17.0

**Minor release.** Two operator-ergonomics features on top of 2.16.1. Cargo SemVer `2.16.1` → `2.17.0`.

### Added

- **`bwoc chat --tmux` auto-starts tmux when needed (#137).** Outside a tmux session it no longer refuses with a "run `tmux new-session` first" hint — it auto-starts (and attaches to) a dedicated `bwoc-<id>` session via `tmux new-session -A` (reattaching if one already exists). Inside a session it still adds a `new-window` as before. A missing `tmux` binary now gives a clear install hint.
- **`bwoc jira` resolves credentials from `.bwoc/secrets.toml` (#135, #136).** `auth.toml` documented the `[jira]` table of the gitignored `0600` `.bwoc/secrets.toml` as credential resolution option 2, but the CLI only read the `BWOC_JIRA_*` env vars — so a secrets-file workspace got `auth: NOT configured`. The documented fallback is now wired (env still wins): fail-closed on an absent/unparseable/group-or-world-accessible file, empty values ignored, token never logged or serialized (reaches the plugin via inherited env only).

## [v2026.5.31-0] — 2026-05-31 — 2.16.1

**Patch release.** Plugin-install + integration-plugin bug fixes on top of 2.16.0, mostly from field reports against the `v2026.5.30-0` build. Cargo SemVer `2.16.0` → `2.16.1`.

### Fixed

- **`bwoc plugin install` accepts every declared plugin kind (#126, #127).** The installer validated `[plugin].kind` against a stale four-kind list (`memory-backend, llm-backend, workflow, audit`), so the bundled reference plugins for the five kinds added since (`jira`, `okr`, `council`, `figma`, `gws`) could not be installed even though they pass `bwoc check`. `validate_plugin_kind` now shares the canonical `check::PLUGIN_KINDS` list, so installer and checker can't drift; the `bwoc check` "expected one of …" messages now derive from the same list too. Test-installing all 19 bundled plugin roots: 11/8 → 19/0.
- **`jira-cloud-rest` query verb works against live Jira Cloud (#128, #129, #132).** The `query` verb called `GET /rest/api/3/search`, which Atlassian removed (HTTP 410, CHANGE-2046) → migrated to the token-paginated `GET /rest/api/3/search/jql` (projection keeps `total`/`start_at`/`max_results` for schema stability and adds `next_page_token`/`is_last`). Project scoping also lifts a trailing `ORDER BY` outside the `AND (...)` predicate instead of producing invalid JQL. Plugin bumped to `0.1.1`; `SPEC.md`/`SPEC.th.md` updated (EN/TH parity).
- **`bwoc gcloud project list` human output no longer reports `0 project(s)` (#130, #131).** `run_project_list` read the plugin response as a bare array, but the `gcloud-project` plugin returns an object `{ok, operation, total, projects:[…]}` — so the count and rows were always empty (`--json` was unaffected). Now reads `total` + `projects` from the object, matching the sibling `compute`/`storage`/`run` list renderers.

## [v2026.5.30-0] — 2026-05-30 — 2.16.0

**Minor release.** Frontier-model surface + self-hosted runtime model control: `primaryModel: "auto"` runtime selection (#120), Claude Opus 4.8 + GPT-5.5 model surface with a backend-neutral `reasoningEffort` knob (#121), the declared-backend count canonicalized at six (#122), and `bwoc spawn` forwarding the agent's model + reasoning-effort (#123). Also bundles the previously-unreleased CI least-privilege + `release.yml` hardening (#116–#118). Cargo SemVer `2.15.0` → `2.16.0`.

### Added

- **`primaryModel: "auto"` runtime model selection** — an agent's `config.manifest.json` may set `primaryModel` to the literal `"auto"` plus an ordered `autoModels` candidate pool; the harness then resolves a concrete model at run time against the **live** provider rather than pinning one at incarnation. Resolution is a deterministic four-criteria pipeline: availability (`ProviderClient::list_models` ∩ candidates), context fit (`model_context_limit` vs an estimated task-token need), task class (an EN/TH keyword + length heuristic splitting heavy vs light work), and cost (candidate order *is* the cost axis — heavy tasks take the most-capable/most-preferred fitting model, light tasks the cheapest). Scoped to harness backends (`ollama` / `openai-compatible`); vendor CLIs (Claude/Codex/Kimi) self-select, so `"auto"` is a no-op there. The resolver also harvests the remaining available candidates and their probed context limits into the harness's previously-empty `LoopConfig` fallback / token-pressure / context-limit fields. New `bwoc-harness::model_select` module; new `NoAutoCandidate` error when the pool is empty or nothing is reachable. `bwoc new` gains no flag — operators opt in by hand-editing the manifest.
- **GPT-5.5 model-surface refresh** — the `bwoc new` picker now recommends GPT-5.5 for Codex and OpenAI-compatible backends, `bwoc help backends` documents the refreshed GPT-5 family examples, and `docs/{en,th}/HARNESS` shows an OpenAI-compatible `primaryModel: "auto"` pool with GPT-5.5 first. The harness also reads optional `reasoningEffort` from `config.manifest.json` and sends it as `reasoning_effort` on OpenAI-compatible completion requests. The neutrality checker now flags hardcoded GPT-5 family IDs in instruction files, preserving backend-neutral `AGENTS.md`.
- **Claude Opus 4.8 support** — `claude-opus-4-8` is now the recommended Claude model in the `bwoc new` picker catalog and the `bwoc help backends` table, and in the EN/TH incarnation docs + how-to examples. The new `reasoningEffort` manifest field also reaches the **Claude** backend: `bwoc run` appends `--effort <level>` to `claude -p` (Opus 4.8 effort control — `low|medium|high|xhigh|max`) when the agent's manifest sets it. The field is backend-neutral and free-form (value space is backend-specific); backends without an effort control ignore it. Effort lives in the manifest only — no `AGENTS.md` placeholder — since it is runtime dispatch config, not LLM-facing instruction.

### Changed

- **`bwoc spawn` forwards the agent's model and reasoning-effort.** `spawn` for harness backends (`ollama` / `openai-compatible`) now passes the manifest's `primaryModel` as `--model` — previously it execed `bwoc-harness` with no model and silently fell back to the harness default (`gemma4`), so `primaryModel` (including the `"auto"` sentinel) was ignored via `spawn` and only worked via `bwoc run`. Vendor backends with a verified effort flag also honour `reasoningEffort`: Claude → `--effort <level>`, Codex → `-c model_reasoning_effort=<level>`. Kimi (boolean `--thinking` only) and Antigravity (no effort flag) emit a one-line note and pass nothing rather than fabricate a mapping. An explicit `--model`/`--effort` in `--extra` always wins.
- **Declared-backend count canonicalized at six.** The docs described the declared backends inconsistently (`four` for the vendor CLIs, `five` with ollama, `six` after the OpenAI-compatible surface refresh); the code has always had six (`Backend` enum / `bwoc new --backend`: claude, antigravity, codex, kimi, ollama, openai-compatible). Normalized the general declared-count statements and enumerated lists to **six** across `PLUGINS`, `ROADMAP`, `SRS`, `ARCHITECTURE` (EN/TH), `help backends`, `configure-backends`, the plugins README, and the agent-template `AGENTS.md`; `check.rs` `BACKEND_NAMES` (neutrality denylist) gains `openai-compatible`. Narrative/CI-reality statements that correctly say "five" (ollama was the fifth added; CI exercises the harness once) are left as-is.
- **`release.yml` drops the `RELEASE_PAT` path (#116, #117)** — the zero-touch PAT hook (added with the #101 fix) was never wired: the secret was unset, so every formula bump used the `GITHUB_TOKEN` fallback (branch push + manual finish command). Removed the PAT plumbing and the now-unused `pull-requests: write` grant; the `bump-formula` job keeps only `contents: write`. No behavior change — the operator still opens + auto-merges the formula PR by hand, as it already did every release.

### Security

- **Least-privilege `GITHUB_TOKEN` on CI workflows (#118)** — `ci.yml` and `docs.yml` now declare an explicit `permissions: contents: read` instead of inheriting the repo default (currently `read`), so a future flip of the repo/org default to "write" can't silently over-grant a checkout-only job. `pages.yml` and the two `claude*` workflows were already correctly scoped.

## [v2026.5.29-3] — 2026-05-29 — 2.15.0

**Minor release.** gcloud IAM project bindings (EPIC-12, #99) — the **fourth and last** write-capable GCP slice and the first use of the risk matrix's top tier, **T4**. Cargo SemVer `2.14.0` → `2.15.0`.

### Added

- **`bwoc gcloud iam {get, add, remove}` (#99)** — project IAM policy operations via the new `workflow/gcloud-iam` plugin. `get` is **T0** (read; never skill-exposed — a policy read discloses security posture). `add`/`remove` of a `(member, role)` binding are **T4 — refuse-by-default**: they run only when the workspace sets `[plugins.gcloud-iam] writes_enabled = true` **and** the operator clears a typed `member role` confirm. Public principals (`allUsers`/`allAuthenticatedUsers`) are hard-refused; high-privilege roles (`owner`/`editor`/`*.admin`/`iam.*`) are flagged in the prompt. `--json` requires `--yes`. Validators for project id / IAM member / role; `bwoc check` auto-audits the manifest. Deferred: `set-iam-policy`, SA-key minting, custom roles, non-project resource IAM.

### Security

- **IAM writes are gated at the matrix's top tier (T4).** Reversibility (a matching `remove`/`add` undoes a binding) does **not** demote the tier — the exposure window during a bad grant is not undoable, so the blast radius is security. The standing `writes_enabled` opt-in + typed-name confirm are layered on the existing `--`/`=` option-injection guard (#92): member/role reach `gcloud` as `--flag=value`, the project id as a positional after `--`. The plugin reads no credential value (Adinnādāna) and never mints one (no SA-key creation).

## [v2026.5.29-2] — 2026-05-29 — 2.14.0

**Minor release.** gcloud Cloud Run serverless (EPIC-11, #98) — the third write-capable GCP slice, on the EPIC-8 foundation. Cargo SemVer `2.13.0` → `2.14.0`.

### Added

- **`bwoc gcloud run {list, describe, deploy}` (#98)** — Cloud Run service operations via the new `workflow/gcloud-run` plugin. Reads (`list`/`describe`) are unguarded (T0); **`deploy` is T2 — confirm + echoed target** (resolved `service / region / {image|source} / traffic`, since a deploy routes 100% traffic to the new revision but is reversible via revision rollback). `deploy` requires `--service`/`--region` + exactly one of `--image`/`--source` (`--source` canonicalized to an absolute existing dir); service/region names validated before dispatch. `--json` requires `--yes`. Standalone `gcloud-build` and `services delete` are deferred to their own slices.

### Security

- **gcloud-run reads no credential value (Adinnādāna).** It sources the sibling `gcloud-auth` helpers and asks `gcloud` for Cloud Run state; `auth.toml` declares shape only. Operator values reach `gcloud` as `--flag=value` or after a `--` separator (option-injection guard, #92 precedent); the BWOC CLI owns the T2 gate so the plugin runs `gcloud run deploy --quiet`. `bwoc check` auto-audits the plugin manifest.

## [v2026.5.29-1] — 2026-05-29 — 2.13.0

**Minor release.** Google Workspace `gws` plugin kind (#107) + gcloud storage objects (EPIC-10, #97). Cargo SemVer `2.12.0` → `2.13.0`.

### Added

- **`gws` plugin kind + `bwoc gws {auth, drive, gmail, calendar}` (#107)** — a read-mostly Google Workspace integration (the framework's ninth plugin kind). `gws-auth` owns the OAuth2 credential surface; `gws-drive`/`gws-gmail`/`gws-calendar` source the token from it and project Drive files / Gmail threads / Calendar events into the Workspace Resource Schema. Each plugin ships an EN/TH SPEC pair; `bwoc check` gains a fail-closed `audit_gws_auth` secret-leak guard.
- **`bwoc gcloud storage {list, stat, put, delete}` (#97)** — Cloud Storage object operations via the new `workflow/gcloud-storage` plugin. Reads (`list`/`stat`) are unguarded; `put` is stat-first (T1 new / T2 overwrite, echoing the existing object); **`delete` is T3 — typed-name confirmation** (re-type `gs://bucket/object`), the first irreversible-write tier of the EPIC-9 risk matrix. `--instance`-style validation on bucket/object before dispatch.

### Security

- **OAuth tokens never touch tracked files (gws).** The token is runtime-resolved from `BWOC_GWS_TOKEN` or a `0600` `.bwoc/secrets/gws-token.json`, handed only to the `Authorization: Bearer` header, and never serialized into output (Adinnādāna). `auth.toml` declares shape only; `bwoc check` fails closed on any value-looking field. REST query params are URL-encoded and resource/calendar IDs + queries are validated before dispatch (no injection).
- **gcloud storage writes are tiered by reversibility × blast radius** — `put` stat-first (T1/T2), `delete` T3 (typed-name). Operator values reach `gcloud` as `--flag=value` or after a `--` separator (option-injection guard, #92 precedent).

## [v2026.5.29-0] — 2026-05-29 — 2.12.0

**Minor release.** gcloud compute lifecycle (#96) — the first write-capable GCP slice (EPIC-9), on the EPIC-8 foundation. Cargo SemVer `2.11.0` → `2.12.0`.

### Added

- **`bwoc gcloud compute {list, describe, start, stop}` (#96)** — instance lifecycle via the new `workflow/gcloud-compute` plugin. Reads (`list`/`describe`) are unguarded; `start` is confirmation-gated (T1), `stop` is gated **with the resolved `project/zone/instance` echoed** (T2). `--json` requires `--yes`; `--instance`/`--zone` are required and validated (RFC 1035) before dispatch. Sources the sibling `gcloud-auth` credential helpers; `auth.toml` is shape-only; `bwoc check` audits the plugin.
- **Reusable write-verb risk matrix** — the design note authors the T0–T4 confirmation-tier template (read → reversible/cost → reversible/availability → irreversible/typed-name → security/opt-in) that the remaining GCP slices (storage #97, serverless #98, IAM #99) instantiate.

### Security

- Compute writes pass every operator value to `gcloud` as `--flag=value` or after a `--` end-of-options separator (option-injection guard, #92 precedent), and reject `-`-leading instance/zone ids at the CLI before dispatch. `start`/`stop` mutate remote instances but are reversible; `delete`/`reset`/`create` are deliberately out of scope.

### Fixed

- **`release.yml` no longer fails when `RELEASE_PAT` is unset (#101)** — the Homebrew formula-bump step pushed the branch then failed creating the PR (the org blocks `GITHUB_TOKEN` from opening PRs), turning every release run red. It now exits green and prints the one finish command in the job summary; with `RELEASE_PAT` set it opens + auto-merges the formula PR hands-off.

## [v2026.5.28-1] — 2026-05-28 — 2.11.0

**Minor release.** GCP `gcloud` workflow plugin foundation (#86) — the framework's second `workflow`-kind integration (after `jira`), designed read-mostly-first. Cargo SemVer `2.10.0` → `2.11.0`.

### Added

- **`bwoc gcloud {auth, project, status}` (#86)** — dispatches the `workflow/gcloud-*` reference plugins (no new plugin kind). `auth status`/`login`, `project list`/`show`/`set-default`, and an aggregate `status`. `--json` twins on every verb.
- **Two reference plugins** — `gcloud-auth` (credential **state** only: active source + account email, never the token) and `gcloud-project` (`list`/`show`/`set-default`). Auth precedence ADC → service-account JSON (`.bwoc/secrets/gcloud-sa.json`, gitignored) → `BWOC_GCLOUD_*` env; `auth.toml` declares **shape only, no values**.
- **`gcloud-ops` skill** — the first skill spanning multiple plugins (`whoami`/`current-project`/`switch-project`); `login` excluded (browser-driven). EN/TH SPEC pairs for both plugins + the skill.
- **`bwoc check` audits `workflow/gcloud-*`** — manifest entry path-traversal + an `auth.toml` secret-leak guard (fail-closed, value redacted) + `bwoc skill verify gcloud-ops` resolution.

### Security

- **`auth.toml` carries no credential values** — the plugins never read a secret; `bwoc check` fails closed on any value-looking field (mirrors the jira guard).
- **Write verbs are confirmation-gated** — `project set-default` (local `gcloud` config only) and `auth login` prompt; `--json` requires `--yes`. Project ids are validated (`6–30`, `[a-z0-9-]`, lowercase-first) before dispatch.
- **Option-injection hardening (#92)** — plugin shell-outs pass operator-supplied values to `gcloud` after a `--` end-of-options separator, so a `-`-leading id can never be parsed as a flag.

## [v2026.5.28-0] — 2026-05-28 — 2.10.0

**Minor release.** A2A auth phase (#80, PRs #81–#84, #87) — the follow-up to A2A v1 (#48): the listener is now safe to expose beyond loopback, and the outbound client authenticates to peers. Closes the security deferrals the v1 notes flagged. Cargo SemVer `2.9.0` → `2.10.0`.

### Added

- **Inbound Bearer auth (AP1, #81)** — when a token is configured (`BWOC_A2A_TOKEN` env or the agent's `.bwoc/a2a.token` file), the JSON-RPC + SSE endpoints require `Authorization: Bearer <token>`; the Agent Card GET stays public and advertises the requirement (`securitySchemes`/`security`). No token ⇒ the unchanged loopback-only posture.
- **Webhook delivery (AP3, #83)** — the push-notification delivery deferred in v1 now fires: when auth is on, a watcher POSTs `TaskStatusUpdateEvent`s to registered webhooks (bearer-authed from the stored config), gated by an SSRF egress filter.
- **Outbound client auth (AP5, #87)** — `bwoc a2a send`/`fetch-card` present a per-peer bearer token from `<workspace>/.bwoc/a2a-credentials.json` (origin-keyed, `0600`-gated) or a `--token` override; `send` honors the remote card's declared scheme, presenting the credential only to a peer that declares Bearer.
- **`bwoc a2a serve --allow-unauthenticated` (AP2, #82)** — opt back into serving a non-loopback bind without a token (loud warning), for trusted networks / a front proxy that adds auth.

### Changed

- **A non-loopback `--bind` now refuses to start without auth (AP2, #82)** — previously it warned and served. A token (or `--allow-unauthenticated`) is required to expose the listener beyond loopback; loopback and auth-on binds are unchanged.

### Security

- **Constant-time token comparison** for the inbound Bearer check (AP1, #81); the scheme is matched case-insensitively (RFC 7235).
- **`0600` gate** on secret files read by the listener/client — `.bwoc/a2a.token` (AP1) and `.bwoc/a2a-credentials.json` (AP5) are refused if group/world-readable, with a `chmod 600` remediation.
- **SSRF guard on webhook delivery (AP3, #83)** — webhook URLs resolving to loopback/private/CGNAT/link-local/metadata (`169.254.169.254`)/ULA ranges are rejected; non-loopback must be `https`; the connection is **pinned** to the validated IP so a DNS rebind can't redirect the POST to an internal service.
- **Rate limit + concurrency cap (AP4, #84)** — a global token-bucket request rate limit (`429` + `Retry-After` when exceeded) and a `SubscribeToTask` concurrent-stream cap, applied unconditionally as resource guards for the exposed endpoint.
- **No outbound credential leak (AP5, #87)** — the client never sends a bearer token to a peer whose card declares no auth.

## [v2026.5.27-3] — 2026-05-27 — 2.9.0

**Minor release.** A2A (Agent2Agent) protocol interop — v1 (#48, PRs #71–#77). BWOC agents can now talk to non-BWOC agents over the open A2A 1.0.0 protocol. Cargo SemVer `2.8.0` → `2.9.0`.

### Added

- **`bwoc a2a serve <agent>` (#48)** — run an A2A HTTP listener for a local agent: the Agent Card at `/.well-known/agent-card.json` and a JSON-RPC endpoint. `SendMessage` drops the inbound message into the agent's `inbox.jsonl`. **Loopback-only by default** (no auth yet); a non-loopback `--bind` warns. Per-request body + inbox size caps guard growth.
- **`bwoc a2a card <agent>`** — print the agent's manifest-derived Agent Card.
- **`bwoc a2a fetch-card <url>` / `bwoc a2a send <url> "<text>"`** — outbound client: fetch a remote agent's card, or send it a `SendMessage` (reqwest, `rustls-tls`).
- **A2A `tasks/*`** — `GetTask`/`ListTasks` bridge a team's Saṅgha task list (`bwoc a2a serve --team <id>`); `CancelTask` honestly returns `TaskNotCancelable` (the lead owns task lifecycle).
- **A2A SSE streaming** — `SubscribeToTask` streams a team task's state transitions; `SendStreamingMessage` is an honest single-event stream (BWOC processes asynchronously).
- **A2A push-notification config** — `Create`/`Get`/`List`/`DeleteTaskPushNotificationConfig` manage per-task webhook configs (persisted, `0600`). Webhook *delivery* is deferred to the auth phase (an SSRF/exfil egress under no-auth).
- **New `bwoc-a2a` crate + binary** — the A2A protocol core, listener, client, and config CRUD. `bwoc a2a` execs the `bwoc-a2a` sibling binary so the **HTTP/async stack (axum, tokio, reqwest) never enters `bwoc-cli`'s dependency tree** (the `bwoc-harness` subprocess pattern); `bwoc-core` stays HTTP-free.

### Notes

- A2A v1 is loopback-only and unauthenticated by design. The **auth phase** (authenticated peers, non-loopback bind, per-peer rate + subscription-concurrency caps, push webhook delivery + SSRF guard, outbound signing) is a separate future milestone.

## [v2026.5.27-2] — 2026-05-27 — 2.8.0

**Minor release.** Cross-workspace give-feedback — the write path of #20. Cargo SemVer `2.7.0` → `2.8.0`.

### Added

- **`bwoc peer feedback <agent> "<review>" --from <local-agent>` (#20 / #67)** — deliver a signed `kind: feedback` envelope into a peer agent's inbox across the interconnect mesh (local-FS). Peer-routed (skips the local fast path), **signature-required** (fails at the source if the sender has no key), and no spurious local tmux wakeup. Completes the three peer verbs (view + learn shipped in 2.3.0).

### Changed

- **Trust gate verifies cross-workspace senders (#66).** On a local-registry miss, the `bwoc-agent` trust gate resolves the sender via the recipient's `routes.toml` + the peer's published `signingPublicKey` and verifies the signature, instead of refusing every peer as `unknown_sender`. Read-vs-write split: a cross-workspace write requires a provable signature in `warn` as much as `enforce` (unsigned ⇒ `unsigned_cross_workspace`); `BWOC_SIGNING_MODE=off` remains the global escape hatch.

## [v2026.5.27-1] — 2026-05-27 — 2.7.0

**Minor release.** Installable plugins & skills + ISO-compliance audit plugins. Cargo SemVer `2.6.0` → `2.7.0`.

### Added

- **Installable plugins (#58)** — `bwoc plugin install` (git URL or tarball; first install acknowledged via `--allow-new-source`) + `bwoc plugin list --kind`. Remote installs are gated by a SHA-256 sidecar; a missing sidecar on a git source is **refused** (publish a `.sha256` or pass `--no-verify`) rather than silently passing the gate (BWOC-38).
- **Installable skills (#58)** — `bwoc skill` install/list/verify. The `[gates].verify` command is arbitrary shell from an untrusted manifest, so it is **never executed by default** — static checks only, command printed for inspection; opt in with `--run-gates` (BWOC-37).
- **ISO-compliance audit plugins (#58)** — `bwoc audit run` dispatches `audit`-kind plugins through a strict findings schema (severity/status/evidence enums; exit code = fail count). Ships **ISO 9001** (signed-attestation runtime), **27001 · 20000-1** (honest `not_implemented` stubs), and **29110** (filesystem-evidence runtime), plus a signed-attestation evidence model (`attestation` / `sample` evidence kinds).
- Plugin/skill templates, the `worktree-discipline` skill, and the `memory-tier2-noop` plugin.

### Security

- Plugin/skill `entry` is validated against path traversal before spawn — a manifest cannot point `entry` at an arbitrary host binary (`..`/absolute rejected, BWOC-36).
- Git installs no longer treat a missing checksum sidecar as a verified install (BWOC-38); tarball-slip and git-ref option injection hardened.

## [v2026.5.27-0] — 2026-05-27 — 2.6.0

**Minor release.** `bwoc-harness` v2 (the #39 epic) + ed25519 message authentication. Cargo SemVer `2.5.0` → `2.6.0`.

### Added

- **harness-v2 (#39 / #57)** — durable/resumable runs (per-turn checkpoint + `--resume`, HV2-2), Saṅgha runtime (a lead spawns sandboxed subprocess workers, HV2-1), run-end retrospective (HV2-3), MCP client (HV2-5), per-run budget hard gate (HV2-6), streaming usage + concurrent tool execution (HV2-7).
- **ed25519 message signing (HV2-4)** — new lean `bwoc-signing` crate (RFC 8785 JCS canonical form); `bwoc send` signs envelopes; `bwoc trust --keygen [--all]` generates/backfills keypairs (private key 0600 in `.bwoc/agent.key`, public key in the manifest); the `bwoc-agent` trust gate verifies the signature before the Kalyāṇamitta check — **enforce by default** (`BWOC_SIGNING_MODE`), bad/tampered signatures refused in every mode. Spec: [`SIGNING.en.md`](docs/en/SIGNING.en.md).

## [v2026.5.25-1] — 2026-05-25 — 2.5.0

**Minor release.** Live fleet operations + a self-updating toolchain. Cargo SemVer `2.4.0` → `2.5.0`. CalVer per [VERSION.md policy](VERSION.md#versioning-policy--dual-namespaces).

### Added

- **`bwoc inbox --all --watch` — fleet-wide merged live message stream (#46)** — lifts the prior `--all`+`--watch` refusal (`--clear` stays refused under `--all`) and tails every agent's inbox at once, each new envelope tagged with its recipient in arrival order; `--json` adds a `recipient` field. Reuses the single-inbox tail core (`read_complete_lines_from`) — one watcher, not two.
- **`bwoc dashboard` live agent-activity (#45)** — the TUI dashboard gains a per-agent activity column (working/idle/running/stale) fed by `bwoc sessions` on the existing 2 s tick, plus a detail pane (session state + backend + pid + last-seen) and a capped live log tail. Observe-only; reuses the `sessions` resolver.
- **Startup update-check — opportunistic drift notice (#44)** — released binaries now print a one-line "newer release available" notice (to stderr) on normal use, throttled to ≤1 network check / 24 h via a `~/.bwoc/update-check.json` cache refreshed in a detached background process. Guarded (TTY-only, skips `--json`/piped/`SourceBuild`/the `update` command), opt-out `BWOC_NO_UPDATE_CHECK=1`, silent offline. Closes the stale-install gap first observed in #3.

### Changed

- **Homebrew formula auto-bumps on release (#52)** — `release.yml` gains a `bump-formula` job that rewrites `Formula/bwoc.rb` (version + url tags + sha256 from the release sidecars) and commits it on every release-tag publish, so the tap can never go stale again. Logic lives in `scripts/bump-formula.sh` (locally testable). Manual 2.4.0 catch-up was #51.

### Fixed

- **What's New banner showed the wrong version** — `whats_new` HEADLINE/HIGHLIGHTS were stuck at the 2.3 release, so a 2.4.0 build greeted users with "BWOC 2.3". Updated, and a guard test (`headline_version_matches_build`) now asserts HEADLINE tracks `CARGO_PKG_VERSION` major.minor so it can't silently drift again.

## [v2026.5.25-0] — 2026-05-25 — 2.4.0

**Minor release.** Phase 4's one framework-owned line item lands as a command — `bwoc fleet health` (#35) — and the Windows destructive-command guardrails (#31) close the caveat flagged in 2.3.0's Windows-support entry. Cargo SemVer `2.3.0` → `2.4.0`. CalVer per [VERSION.md policy](VERSION.md#versioning-policy--dual-namespaces). (The `bwoc sessions` monitor (#21) also merged in this window; it was already described in the 2.3.0 entry below.)

### Added

- **`bwoc fleet health [--json]` — Aparihāniya-dhamma 7 governance signals (issue #35)** — turns the [`FLEET-GOVERNANCE.en.md`](docs/en/FLEET-GOVERNANCE.en.md) spec's *stubbed* signals into a real **read-only, report-only** command (no gating — v1 ships signals; v2 may promote to gates once telemetry justifies). One workspace-scoped run reports each of the seven DN 16 non-decline conditions as ✓ / ⚠ / ℹ: **1** regular meetings (agent-dir mtime vs `--stale-days`), **2** coordinated start/end (reuses `doctor` stale PID/socket findings), **4** honor template version, **5** protect vulnerable (inbox-refusal counts) — mechanical; **3** convention drift (`git status .bwoc/` porcelain) and **6** shared-resource authorship (`git` author vs operator) — git-backed mechanical checks (exceeding the v1 informational-only slice); **7** protect senior agents — informational. Orchestrates existing surfaces (registry / `doctor` / `check` / inbox refusals) rather than reimplementing; dep-lean; backend-neutral. 15 unit tests.

### Fixed

- **Windows destructive-command guardrails (issue #31)** — the harness dangerous-path guard was unix-oriented; it now also blocks Windows destructive patterns (`del /s`, `rmdir /s`, `format`, `Remove-Item -Recurse`), closing the caveat noted in the 2.3.0 `bwoc-harness — Windows support` entry. Realises Sīla *Pāṇātipāta* (no destruction) uniformly across shells (Samānattatā).

## [v2026.5.24-1] — 2026-05-24 — 2.3.0

**Minor release.** The plugin-system cycle (#6) — a real OS-level sandbox (landlock / `sandbox-exec`, replacing the stub), `bwoc-harness` Windows support, an OpenAI-compatible provider + vetted-model mode, cross-workspace `bwoc peer` view/learn, the `bwoc sessions` monitor, Trust v2 warn-mode, the document-kind mechanism, per-model token-limit auto-switch, and `bwoc run` / `bwoc update`. Cargo SemVer `2.2.0` → `2.3.0`. CalVer per [VERSION.md policy](VERSION.md#versioning-policy--dual-namespaces).

### Added

- **`bwoc run <agent> --task` — headless single-task invocation (issue #5)** — runs an agent on one task with no interactive session: the `claude` backend shells `claude -p`, `ollama` routes through `bwoc-harness`, and `codex` / `agy` / `kimi` return `HeadlessUnsupported` rather than failing silently. A `CommandRunner` trait keeps the path unit-testable offline (mock runner). Closes the "agents aren't headlessly runnable" gap that blocked autonomous orchestration.
- **Tier 2 pluggable deep-memory backend** — a `DeepMemory` trait (`wake_up` / `search` / `mine`) in `bwoc-core` with a `ShellDeepMemory` reference impl (shells the `deepMemoryCmd` from `config.manifest.json`) and a `DisabledDeepMemory` no-op when Tier 2 is unconfigured; surfaced as `bwoc memory wake-up | t2-search <query> | mine <path>`. Realises AGENTS.md §7.2 — the optional deep-memory tier whose absence never breaks the agent.
- **`bwoc update` — release-drift detection + delegate-only upgrade (issue #8)** — `bwoc update --check` is a read-only check comparing the binary's embedded Release CalVer (`option_env!("BWOC_RELEASE_CALVER")`, baked in by `release.yml`) against the latest GitHub release tag (up-to-date / update-available / source-build). Honours the [VERSION.md policy](VERSION.md) that *CalVer is the public release identity* (not SemVer). Plain `bwoc update` detects the install method and **delegates** the upgrade: Homebrew → `brew upgrade bwoc`, cargo → `cargo install --git …`, raw binary → points at the release page (no self-swap). Prints the command by default; `--run` executes the delegated package-manager command. Stays dep-lean — no HTTP client; shells `gh` / `curl` behind a `CommandRunner` seam (offline-unit-tested, 26 tests). **Self-replacing a raw binary is intentionally deferred** (destructive — Sīla *Pāṇātipāta* — and never done on uncertainty). Pairs with the #3 startup drift guard.
- **Workspace document-kind mechanism — `bwoc notes | retro | research` (epic #12, subsumes #10/#11)** — one generic engine over a `bwoc-core::doc_kind` registry: each kind (`notes`, `retrospectives`, `research`) is a `DocKind { dir, committed, template }`, and `bwoc <kind> new|list|view` scaffolds `<dir>/YYYY-MM-DD_<slug>.md` (refusing to clobber), lists newest-first, and views by date/name. Templates are framework-grounded — notes = the CLAUDE.md log skeleton, retrospectives = Paññā-3 (Sutamayā/Cintāmayā/Bhāvanāmayā), research = Question/Scope/Sources/Findings/Recommendation. Bilingual `NAMING` rows added for the two new dirs. dep-lean; one code path, no per-kind duplication. Extended (#18) with **workspace-declared custom kinds** (`.bwoc/doc-kinds.toml` + a generic `bwoc doc <kind>` command) and **retro metrics-prefill** (summarises `session-metrics.jsonl` into the retrospective's `## Metrics` section).
- **`bwoc-harness` — per-model token-limit checker + auto-switch (issue #13)** — the agentic loop now tracks a per-model context limit (`LoopConfig.model_context_limits`); when the running context nears the *active* model's limit it switches to a configured larger-context model from `token_pressure_models` (if one passes the vetted-model gate) **before** falling back to compaction — escalate-only, no history loss. A distinct trigger from the error-based `fallback_models` chain; recorded separately in telemetry (`token_pressure_switches`). Backend-neutral, dep-lean. Per-model limits can also be **provider-queried** (#19) — Ollama `/api/show` `num_ctx`, cached per model — when not set in static config (precedence: static → queried → default).
- **Trust v2 — warn-mode refusal (`off` / `warn` / `refuse`) (issue #6 / WS5)** — the inter-agent trust gate gains an explicit per-recipient `mode` (manifest `trust.mode`): `warn` lets an envelope from a sender missing a required Kalyāṇamitta quality **pass** while emitting a `trust_warn` log line, instead of refusing it. Backward-compatible — a manifest without `mode` keeps v1 semantics exactly (empty `requiredTrust` → off, non-empty → refuse); `warn` is opt-in, no silent demotion. Realises `trust.md` §Refusal modes. (Cryptographic signed envelopes remain deferred — see above.)
- **`bwoc peer` — read-only cross-workspace view + learn (issue #20)** — `bwoc peer list` shows peers declared in `.bwoc/interconnect/routes.toml`; `bwoc peer status <key>` reads (read-only, local FS) a peer's agents (`AgentsRegistry`) + Saṅgha open tasks; `bwoc peer learn <key>` reads a peer's **allowlisted** shared docs (the peer opts in via `.bwoc/interconnect/shared.toml`; path-containment enforced) (#26). Reuses existing loaders pointed at the peer root — no new parsing/deps. *Give-feedback* (write, needs cross-workspace identity) stays deferred. Realises Oracle's cross-mesh state-sensing — **Kalyāṇamitta / Samānattatā / Anattā** (no central broker).
- **`bwoc sessions` — discover + monitor agent sessions (issue #21)** — `bwoc spawn` drops a `.bwoc/sessions/<agentId>.json` marker (backend / pid / startedAt / tmux); `bwoc sessions` reads markers (pid-liveness via `libc::kill`, stale markers cleaned) plus a process/tmux **scan fallback** (behind a mockable seam) for unmarked backend processes, reporting backend / agent / pid / state / source. Observe-only (never drives a session); backend→process map in one place (Samānattatā); dep-lean.
- **`bwoc-harness` — OpenAI-compatible provider + vetted-model mode (issue #6 / WS4)** — `Backend::OpenAiCompatible` runs any OpenAI-compatible endpoint (vLLM / LM Studio / llama.cpp / remote) via a `baseUrl` manifest field passed to the harness `--endpoint` (`OPENAI.md → AGENTS.md` symlink); the provider client is unchanged. `--vetted-mode off | warn | enforce` (default `warn`, backward-compatible) controls an unvetted model — `enforce` refuses an unvetted primary model before turn 1. dep-lean (no new crate).
- **`bwoc-harness` — real OS-level sandbox (issue #6 / WS2)** — replaces the OsSandbox stub: **landlock** (Linux ≥ 5.13 — a `pre_exec` ruleset restricting filesystem writes to the worktree) + **sandbox-exec** (macOS SBPL profile, canonical-path-confined). A factory selects by OS; **graceful-degrade** to the worktree-allowlist on unsupported kernels (never hard-fails). Defence-in-depth over the existing `confine_path`. The `landlock` crate is a Linux-target dep in `bwoc-harness` only.
- **`bwoc-harness` — Windows support (issue #6 / WS7)** — a cross-platform `shell_command` (`sh -c` on Unix, `cmd /C` on Windows) replaces the `sh`-only shell-outs, and the harness is **re-enabled in Windows CI** (workspace now tested uniformly on ubuntu / macos / windows). Caveat: the dangerous-path guardrails are still unix-oriented — Windows-specific destructive patterns (`del /s`, `rmdir /s`, `Remove-Item -Recurse`) are tracked as **#31**.

### Fixed

- **`bwoc new` left `AGENTS.md` placeholders unsubstituted (issue #4)** — the scaffolder now substitutes every `config.manifest.json` placeholder into the generated `AGENTS.md` (and adds `--primary-capability`), so a freshly-created agent is backend-neutral-clean with no leftover `{{…}}` tokens.

## [v2026.5.24-0] — 2026-05-24 — 2.2.0

**Minor release.** Phase 3 (*vaya + interconnect*) — inter-workspace routing, worktree lifecycle, and `bwoc retire` full vaya — plus the new **`bwoc-harness`** self-hosted agentic runtime (run Ollama / any OpenAI-compatible model as a full BWOC agent; Unix-first in v1), and the Windows-CI TOML fix + `actions/checkout` v6 bump. Cargo SemVer `2.1.0` → `2.2.0`. CalVer per [VERSION.md policy](VERSION.md#versioning-policy--dual-namespaces).

### Added

- **Inter-workspace routing — Phase 3 Track A** — `.bwoc/interconnect/routes.toml` lets `bwoc send` reach an agent in a *peer* workspace with no central broker. `bwoc-core::routing` adds a `Routes` type (peer-declared `agent` xor `namespace` → workspace root) and a resolve order: exact `agent` → longest `namespace` prefix → `NotFound`. `send` consults it only after a local-registry miss, so the local-delivery path is byte-for-byte unchanged. Composes with the Kalyāṇamitta-7 trust gate — a cross-workspace sender resolves as `unknown_sender` and is refused — so routing ships ahead of Trust v2. Spec: [`modules/agent-template/interconnect/routing.md`](modules/agent-template/interconnect/routing.md) (+ `.th.md`); mapped to **Anattā** (SN 22.59): no central self, no central broker. Delivers the "coordinate without a central authority" half of the Phase 3 DoD.
- **Worktree lifecycle — Phase 3 Track B** — a `git_worktree` shell-out util (no `git2`/`gitoxide`) plus a `task-claimed` Saṅgha hook that fires `git worktree add <worktreeBase>/<agentId>/<taskId> -b agent/<agentId>/feat/<taskId>` when an agent claims a task. The Saṅgha `Task` struct is **not** extended — worktree location follows the `<worktreeBase>/<agentId>/<taskId>` path convention so retire cleanup is deterministic without parsing any agent log.
- **`bwoc retire` full vaya — Phase 3** — retire now ends an agent's life cleanly: worktree cleanup (worktrees under `<worktreeBase>/<agentId>/` removed via the git util), branch release (`agent/<agentId>/*` — `-d`, escalating to `-D` with the forced branch names surfaced in human + `--json` output), and interconnect deregister (`Routes::remove_agent_routes` strips routes whose `agent` targets the retiree from `routes.toml`). Idempotent and respects the existing file-mode flags. Completes the "an agent's life ends cleanly" half — **the Phase 3 DoD is now met.**

**`bwoc-harness` — self-hosted agentic runtime (issue #1, P1–P5)**

- **New crate `crates/bwoc-harness`** — OpenAI-compatible model-API client + agentic loop runtime for self-hosted / local LLM backends (Ollama first; any `/v1/chat/completions` endpoint). Heavy deps (tokio, reqwest, keyring) are quarantined inside this crate; `bwoc-cli`/`bwoc-agent`/`bwoc-core` stay dep-lean — the zero-dep orchestrator guarantee holds for the default path. Spec: [`docs/en/HARNESS.en.md`](docs/en/HARNESS.en.md) (+ `.th`).
- **Safety guardrails (P2)** — hard, non-overridable engine mapping Sīla 5 + Taṇhā 3: blocks `rm -rf` repo/worktree root, secret writes (PEM/PAT/AWS/credential patterns), identity spoofing, `--no-verify`/force-push, `sudo`/`su`. Denials are fed to the model as tool results — the loop never panics on a denial.
- **Permission system (P2)** — per-tool / per-pattern `allow | ask | deny` from `.bwoc/harness-policy.toml`; `ask` on non-TTY/autonomous fails-safe to `deny`; no policy file = deny-all.
- **Sandbox (P2)** — worktree-confined fs writes (symlink-escape detection), `run_command` cwd-locked + env-scrub + arg-level scan. OS-level isolation (`sandbox-exec`/landlock) is a **pluggable stub** in v1 — worktree+allowlist only.
- **Tool-auth broker (P3)** — scoped credentials from the OS keyring injected into the child env at exec only; never in prompt, log, or telemetry.
- **Task queue (P3)** — async bounded cancellable queue integrating `bwoc-core::team` (Saṅgha); one task in-flight per worktree; `unclaim` rollback on rejection.
- **Telemetry (P3)** — per-turn metrics → `session-metrics.jsonl` (additive to AGENTS.md §8b); OpenTelemetry behind `--features otel` (stub by default).
- **Eval framework (P4)** — offline fixture runner (`task.toml` + `seed/` + `expected/`, rubric scoring); CI tests use a mock provider (no live model). Feeds Paññā 3 triggers.
- **Loop hardening (P4)** — exponential-backoff retry, fallback-model chain, warn-only vetted-model gate, context compaction (truncate-with-marker).
- **Full tool set** — read/write/edit_file, list_dir, grep, run_command, git, run_gates, bwoc_task, bwoc_send, memory_read/write — every tool routed through the guardrails → permission → sandbox pipeline.
- **Backend wiring (P5)** — `bwoc spawn --backend ollama` execs the `bwoc-harness` binary; `OLLAMA.md → AGENTS.md` template symlink.
- **Live-validated 2026-05-23** — end-to-end against real Ollama (`gemma4:latest`): the loop created and ran a correct file; with no policy it correctly denied the write (fail-safe) and fed the denial back to the model. **v1 caveat:** OS-level sandbox is a stub; treat unvetted models + permissive policies with care.

### Fixed

- **Windows CI — routing tests** — the routing peer tests built `routes.toml` by interpolating a temp path into a double-quoted TOML basic string; on Windows the backslashes (`C:\…`) parsed as invalid escapes and failed 3 tests. Switched to single-quoted TOML *literal* strings, which preserve paths verbatim on every platform.

### Changed

- **CI — `actions/checkout` v4 → v6** — checkout v6 runs on Node 24 natively; the `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24` env still covers the remaining JS actions. Removes the Node 20 deprecation banner ahead of the runner cutover.

## [v2026.5.23-3] — 2026-05-23 — 2.1.0

**Minor release.** Saṅgha v1 (agent teams + shared task list, daemon task-watch, opt-in auto-claim, plan-approval Pavāraṇā, blocking task hooks), the single trunk-based branching standard, the "What's New" CLI surface, and dashboard single-agent lifecycle hotkeys. Cargo SemVer `2.0.94` → `2.1.0`. CalVer per [VERSION.md policy](VERSION.md#versioning-policy--dual-namespaces).

### Added

- **Saṅgha Phase B — daemon task-watch** — `bwoc-agent --serve` now watches the shared task lists of every team its agent belongs to and announces newly-claimable tasks (`pending` + deps complete) to stderr: `bwoc-agent: task available ← <team>/<task>: <title>`. Snapshots open tasks at startup (no replay), polls on a 2s cadence, inert when the agent is on no team. **Opt-in wakeup** (`BWOC_TASK_WAKEUP=1`) additionally pings the agent's tmux session with a `[bwoc task <team>/<id>] <title>` marker so a live Claude session can `bwoc task claim` — the agent stays in control (no auto-claim, no stranding). Auto-claim and task hooks deferred to Phase B+. New `bwoc-agent::task_watch` (4 tests). See `modules/agent-template/interconnect/sangha.md` §Phase B.
- **Saṅgha plan approval — Pavāraṇā (Phase C)** — a task can require lead sign-off on a plan before completion. `bwoc task add … --requires-plan` gates the task; `bwoc task plan <team> <task> --as <agent> --plan …` (or `--plan-file`) submits/revises (claimant only) and `bwoc task plan <team> <task>` shows it; `bwoc task approve` / `bwoc task reject` are the lead's verdict (no `--as` — the human is the lead). `bwoc task complete` is refused until the plan is approved — the gate lives in `bwoc-core::team::complete_task` so it holds across every surface. Non-plan tasks are unaffected (opt-in per task). 5 core tests; live-verified the full submit → reject → resubmit → approve → complete cycle. Saṅgha is now feature-complete (A → B/B+ → hooks → C).
- **Saṅgha auto-claim (opt-in)** — `BWOC_AUTO_CLAIM=1` closes the autonomous-teamwork loop: when `bwoc-agent --serve`'s task-watch sees a new claimable task it claims it for its agent (via the locked `bwoc task claim` CLI path — lost races just log) and wakes the agent to work it. Riskiest mode (daemon mutates shared state), gated separately from `BWOC_TASK_WAKEUP`, off by default. Live-verified: daemon auto-claimed a task added while running. Full loop: add → daemon sees → claims → wakes.
- **Saṅgha task hooks** — optional workspace-level shell hooks `<ws>/.bwoc/hooks/task-created` + `task-completed` fire on `bwoc task add` / `complete` (mirrors Claude Agent Teams' TaskCreated/TaskCompleted). Context arrives as env vars (`BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_TASK_TITLE`, `BWOC_AGENT`); a non-zero exit **blocks** the operation (task file unchanged, hook stderr surfaced, exit 2). Missing/non-executable hook = silent no-op. Use for quality gates (e.g. a `task-completed` hook that runs tests). 1 unit test; live-verified pass + block on both events.
- **Online docs link in the CLI** — the bare-`bwoc` banner and `bwoc help` index now surface <https://bemindlabs.github.io/BWOC-Framework/>.

**"What's New" surface**

- **Banner** (bare `bwoc`) gains a `✨ What's New` section listing the current release's highlights.
- **Once-per-version upgrade notice** — any subcommand prints a one-line "you upgraded" notice to stderr the first time it runs on a new `MAJOR.MINOR` (keyed on `~/.bwoc/last-seen-version`, so patch churn doesn't spam). Silent on non-TTY / piped / `--json` output; suppress with `BWOC_NO_WHATSNEW=1`. Highlights live in `crates/bwoc-cli/src/whats_new.rs` (single source; the banner imports them).

**Saṅgha v1 Phase A — teams + shared task list**

- **`bwoc-core::team`** — `Team` (TOML membership) + `Task`/`TaskState` (JSONL) with pure transition rules: `add_task` (dup + unknown-dep rejection), `claim_task` (pending + all-deps-completed → in_progress + claimant), `complete_task` (in_progress + claimant-only → completed). 11 unit tests.
- **`bwoc team create/list/retire`** + **`bwoc task add/list/claim/complete`** — a team groups a subset of workspace agents under one shared task list; teammates self-claim with `--as <agent>` (member-guarded). Dependency-free `O_EXCL` advisory lock (PID + signal-0 staleness steal) serializes claims so two agents never claim the same task; atomic tmp+rename writes; `--json` on every command. Human operator is the implicit lead (no `lead` field).
- **`interconnect/sangha.md` + `.th.md`** — bilingual spec mapping **Saṅgaha-vatthu 4** (team-cohesion norms) + **Saṅghakamma** (the lock-settled claim) to the model. Daemon task-watch, plan approval (Pavāraṇā), and a dashboard task pane are deferred to Phase B/C. See [`notes/2026-05-23_sangha-v1-phase-a.md`](notes/2026-05-23_sangha-v1-phase-a.md).

**Dashboard single-agent lifecycle hotkeys**

- **`s` (start)** — runs the selected agent from the TUI: flips registry status to active and spawns `bwoc-agent --serve`. Shells out to `bwoc start <id> --yes --json` with captured output (TUI-safe), parses `daemon_pid` / `already_running` into the footer, refreshes so status + ●/○ flip. See [`notes/2026-05-23_dashboard-start-hotkey.md`](notes/2026-05-23_dashboard-start-hotkey.md).
- **`x` (stop)** — stops the selected agent (signal the daemon + flip status stopped). Parses `bwoc stop --json`'s `daemon_outcome` enum into a precise footer message. The dashboard now covers the full single-agent lifecycle: chat (`t`/`g`), log (`l`), inbox (`i`), start (`s`), stop (`x`), refresh (`r`). See [`notes/2026-05-23_dashboard-stop-and-start-race-fix.md`](notes/2026-05-23_dashboard-stop-and-start-race-fix.md).

### Changed

- **Single trunk-based branching standard** — consolidated three divergent branch-naming conventions (template `AGENTS.md` §4, `conventions.md`, root `CONTRIBUTING.md`, and SRS FR-4.7 in EN+TH) into one trunk-based / GitHub Flow standard: `main` is the only long-lived branch; topic branches are `<type>/<slug>` where `type` ∈ the Conventional Commit vocabulary (`feat fix docs refactor test chore perf style ci`); the multi-agent collision guard prefixes `agent/<agent-id>/`; no `release/*` or `hotfix/*` branches (CalVer tags cut directly on `main`); branches are deleted after merge (Anattā). See [`notes/2026-05-23_branching-standard-and-team-personas.md`](notes/2026-05-23_branching-standard-and-team-personas.md).

### Fixed

- **`bwoc start` duplicate-daemon race** — `spawn_daemon` now writes `.bwoc/agent.pid` from the parent (with the child's pid) immediately after spawn instead of waiting for the daemon's own startup write. A second `bwoc start` arriving in that window previously read no pid file and spawned a duplicate daemon; it now correctly reports `already_running`.

### Security

- **Dependabot `time` DoS (GHSA-r6v5-fh4h-64xc)** dismissed as not-affected — `time` reaches BWOC only transitively via ratatui-widgets (TUI formatting); the DoS is in time's parsing of untrusted strings, which BWOC never does. Fix (0.3.47) requires Rust 1.88 vs the MSRV 1.85. See [`notes/2026-05-23_time-cve-triage.md`](notes/2026-05-23_time-cve-triage.md).

## [v2026.5.23-2] — 2026-05-23 — BWOC 2.0

**First major version of the BWOC framework.** Significant capability stack on top of the v2026.5.23 baseline; one BREAKING backend rename (`gemini` → `antigravity`/`agy`). Cargo SemVer jumps `0.1.721` → `2.0.0` to mark the discontinuity. CalVer per [VERSION.md policy](VERSION.md#versioning-policy--dual-namespaces).

### Highlights

- **Kalyāṇamitta-7 trust system** — spec v1.1 + 4 implementation steps; permissive by default, opt-in gating via `BWOC_TRUST_GATING=1`.
- **Agent → agent messaging** (Sammā-vācā Phase 1) — `--from` flag + Sāraṇīyadhamma 6 norms in `interconnect/messaging.md`.
- **Inbox tmux wakeup + Stop-hook auto-reply** — sub-second turn latency; `messageId` always, `replyTo` optional.
- **Phase 4 fleet governance spec** (Aparihāniya-dhamma 7, DN 16) — operator-facing.
- **Dual-mode `bwoc check`** — distinguishes template from incarnation; closes silent-pass bug for un-personalized agents.
- **`bwoc chat --ghostty`** + dashboard `g` hotkey for the new-window launcher.
- **HITL cleanup pass** — `bwoc status --banner`, dashboard refusal badge, `start`/`stop` non-TTY consistency, Stop-hook failure surfacing.
- **Auto-version hook** gains minor/major sentinel support via `scripts/queue-bump.sh`.

### Added

**Inbox tmux wakeup + Stop-hook auto-reply (ported from `it-app-workspace/bin`)**

- **Envelope schema** — `inbox.jsonl` envelopes now carry `messageId` (always, format `msg-YYYYMMDDTHHMMSSZ-<5hex>`) and optional `replyTo`. Both fields are additive — `serde_json::Value` readers in the daemon and `bwoc inbox` ignore them silently, so no behavior change for existing flows. Mattaññutā — required-field set unchanged.
- **`bwoc send` flags** — new `--reply-to <msg-id>` threads a reply; new `--no-wakeup` skips the tmux ping for CI/daemon callers. Env opt-out `BWOC_DISABLE_TMUX_WAKEUP=1` for process-wide suppression (used by tests).
- **Native tmux wakeup** — after a successful inbox append, `bwoc send` attempts `tmux send-keys -t <bare-name>` of the marker `[bwoc inbox <msg-id> from <sender>] <message>`. Two-step submit (text → 200 ms → Enter) for the Claude TUI input quirk. Silent skip when tmux is absent or no session matches — daemon poll remains the source-of-truth delivery path.
- **Stop-hook auto-reply** — `modules/agent-template/.claude/hooks/inbox-auto-reply.sh` (new) is a Claude Code Stop hook: reads transcript, detects the inbox marker in the last user prompt, posts the last assistant text back to the original sender with `--reply-to`. Wired via `modules/agent-template/.claude/settings.json` (also new). Backend neutrality: hook is Claude-specific by event-surface; analog paths for AGY / CODEX / KIMI deferred — protocol is shared.
- **Docs** — `modules/agent-template/interconnect/messaging.md` + `.th.md` gain §Envelope Schema field table, `--reply-to` / `--no-wakeup` CLI rows, and a new §Wakeup & Auto-Reply explaining the two-half design (native tmux + Stop hook) plus the per-backend deferral matrix.

See [`notes/2026-05-23_inbox-wakeup-and-auto-reply.md`](notes/2026-05-23_inbox-wakeup-and-auto-reply.md).

### Changed — BREAKING

**Backend rename: `gemini` → `antigravity` (CLI `agy`)**

- Google's Gemini CLI stops serving Google One / unpaid tiers on 2026-06-18 and the replacement coding CLI is **Antigravity** (`agy`), a multi-vendor router exposing Gemini, Claude, and GPT-OSS model families through one binary. Per [Samānattatā](modules/agent-template/docs/en/PHILOSOPHY.en.md), the framework follows the actual product surface — backend `gemini` is replaced by backend `antigravity` everywhere.
- **Rust** (`crates/bwoc-cli`): `Backend::Gemini` → `Backend::Antigravity`, `cli_name()` returns `"agy"`, model list now covers `gemini-3.5-flash-*`, `gemini-3.1-pro-*`, `claude-{sonnet,opus}-4.6-thinking`, `gpt-oss-120b-medium`. All backend-symlink arrays (`check.rs`, `doctor.rs`, `status.rs`, `new.rs`, `dashboard.rs`) swap `GEMINI.md` → `AGY.md`. `bwoc check` `BACKEND_PHRASES` now flags `Antigravity will/can` (not `Gemini will/can`); `HARDCODED_MODELS` gains `gemini-3`, `gpt-oss`. 115 tests pass.
- **Symlinks**: `GEMINI.md` deleted in `modules/agent-template/`, `agents/agent-pi/`, `agents/agent-oracle/`. `AGY.md → AGENTS.md` created in their place.
- **Shell scripts**: `incarnate.sh` and `check-agent-neutrality.sh` updated to create / validate `AGY.md`; `HARDCODED_MODELS` and `BACKEND_PHRASES` mirror the Rust audit.
- **Docs (EN + TH parity)**: VISION, README, SECURITY, ARCHITECTURE, INCARNATION, WORKSPACE at root; AGENTS.md, README.md, CLAUDE.md, conventions.md, neutrality.md, persona/README.md, OVERVIEW, SRS, plugins/README in `modules/`. All `GEMINI.md` → `AGY.md`, "Gemini CLI" → "Antigravity CLI", `backend = "gemini"` → `backend = "agy"`. Model identifiers in `gemini-*` form stay (still the model family; only the routing CLI changed).
- **Migration**: existing agents with `GEMINI.md` symlinks remain functional only until `bwoc check` runs — the audit now expects `AGY.md`. Rename with `mv GEMINI.md AGY.md` or run `bwoc new --force` to regenerate. Existing `.bwoc/agents.toml` entries reading `backend = "gemini"` will fail to parse (no `Backend::Gemini` variant); edit to `backend = "agy"`.

See [`notes/2026-05-23_antigravity-rename.md`](notes/2026-05-23_antigravity-rename.md).

**Kalyāṇamitta-7 trust — all 5 implementation steps shipped**

- **Trust spec v1.1** (`docs(spec)` `f815dbe`) — `modules/agent-template/interconnect/trust.md` + `.th.md` revised to incorporate Oracle + Pi review feedback on the v1 draft shipped 2026-05-23.
- **Step 1 — core** (`feat(core)` `1c54cbc`) — `bwoc-core::Manifest` gains `TrustBlock` + `TrustDeclared`. Manifests now deserialize a `trust` section (7 booleans + optional `requiredTrust` array) with permissive defaults; existing manifests load unchanged.
- **Step 2 — check** (`feat(check)` `ce3907f`) — `bwoc check` verifies Kalyāṇamitta-7 evidence: each declared trust boolean is cross-checked against the matching repo signal so the manifest cannot lie about itself.
- **Step 3 — trust read** (`feat(cli)` `cd10a52`) — new `bwoc trust <agent> read` reports the declared trust block for an agent in the workspace; foundation for the step-4 inbox refusal gate.
- **Step 4 — daemon refusal** (`feat(agent)` pending) — `bwoc-agent --serve` refuses inbox envelopes from senders missing required trust qualities, behind `BWOC_TRUST_GATING=1` env opt-in (v1 safety). Refusals are written to a sidecar `.bwoc/inbox.refusals.jsonl` (never modifying the original envelope — append-only auditability); `bwoc inbox` joins the sidecar at read time so `jq '.[] | select(.refused)'` works verbatim. `from=user` always passes per spec. New `bwoc-core::time` module promoted from `bwoc-cli::util` to share UTC ISO 8601 between CLI + agent. 19 new tests. See [`notes/2026-05-23_trust-step-4.md`](notes/2026-05-23_trust-step-4.md).
- **Step 5 — this CHANGELOG roll-up.** Trust feature complete behind opt-in; v2 (warn-mode, identity proof) is a separate ROADMAP item.

**`bwoc check` becomes dual-mode (template vs incarnation)**

- **Mode detection** (`feat(check)` pending) — `bwoc check` now reads `config.manifest.json::name` to decide whether the target is the template (placeholder name like `{{name}}`) or an incarnated agent (real name). Template mode keeps the existing behavior (asserts placeholders + neutrality rules hold). Incarnation mode asserts the opposite: NO `{{xxx}}` placeholders survive (except `{{taskId}}`, whitelisted as runtime per Appendix A) AND skips the hardcoded-model / hardcoded-tool / backend-phrasing neutrality checks (those guard the scaffold, not the per-agent commitment). Fixed the latent bug where an incarnated-but-not-personalized agent silently passed `bwoc check`. 9 new tests. See [`notes/2026-05-23_check-dual-mode-and-personalize.md`](notes/2026-05-23_check-dual-mode-and-personalize.md).

**Agent personalization**

- **`agents/agent-pi/` + `agents/agent-oracle/` personalized** — placeholders in AGENTS.md + persona/README.md substituted from manifest values (mechanical) + persona-level fields filled with concrete content (`primaryCapability` / `scopeDescription` / `outOfScope` / `moduleName`). Pi = Rust implementation across `bwoc-*` crates; Oracle = fleet coordination via inbox/messaging. Template-only Appendix A (Placeholder Reference) + Appendix B (Quick-Start Checklist) removed from the incarnated agents — those docs apply pre-incarnation only. Both agents now pass `bwoc check` with 0 violations.

**Agent → agent messaging — Sammā-vācā Phase 1**

- **`bwoc send --from <agent>` flag** (`feat(cli)` pending) — `bwoc send` gains an optional `--from <agent>` flag so an envelope can carry a real sender identity (not just `from: "user"`). The named sender must exist in the workspace registry; unknown sender → exit 2 with `SenderNotFound`. Trust verification stays at the recipient daemon (already implemented in trust step 4) so this iter is purely sender-identity plumbing. Backward compatible: omitting `--from` writes `from: "user"` exactly as before.
- **`interconnect/messaging.md` + `.th.md`** — new spec covering the envelope schema, `--from` resolution rules, and **Sāraṇīyadhamma 6** (AN 6.11–12) mapped to engineering rules (API stability, kindly speech, charitable interpretation, observability, common Sīla baseline, shared philosophy graph). Norms only — `bwoc check` does not gate them; the spec exists so an incarnated agent can internalize them.
- **Live verified** — scenario A: sender lacks required qualities → daemon refuses + sidecar log + `jq 'select(.refused)'` matches; scenario B: sender declares qualities → passes silently, no sidecar. See [`notes/2026-05-23_agent-to-agent-messaging.md`](notes/2026-05-23_agent-to-agent-messaging.md).

**Phase 4 — Fleet governance spec (Aparihāniya-dhamma 7)**

- **`docs/en/FLEET-GOVERNANCE.en.md` + `.th.md`** — new framework-root operator-facing spec. Seven non-decline conditions from DN 16 (Mahāparinibbāna Sutta, §1.4 — the Vajjī teaching) mapped to workspace-level fleet operations: (1) regular meetings → `bwoc list` cadence; (2) coordinated start/end → `bwoc doctor` + `bwoc workspace prune`; (3) process-bound convention change → `schemaVersion` discipline; (4) honor template version → `bwoc check --all` version-lag flag; (5) protect vulnerable → respect recipient refusals, don't relax `requiredTrust`; (6) honor shared resources → `agents.toml` + `workspace.toml` + template are operator-owned; (7) protect senior agents → audit trust-dependency before `bwoc retire`. Each condition lists an observable signal (existing query) and a suggested operator practice. v1 is descriptive (signals, not gates); v2 may promote signals to gates as telemetry justifies. **Phase 4 is structurally an ecosystem-viability phase** (external-adoption goals); this spec closes the one Phase-4 line item the framework itself owns. PHILOSOPHY.en.md / `.th.md` cross-reference updated to point to the new location. ROADMAP §Phase 4 gains a "Shipped" subsection. See [`notes/2026-05-23_phase-4-fleet-governance.md`](notes/2026-05-23_phase-4-fleet-governance.md).

**`bwoc chat --ghostty` + dashboard `g` hotkey**

- **`bwoc chat --ghostty <name>`** (`feat(cli)` `5110dde`) — new flag opens a fresh Ghostty terminal window running `bwoc spawn` for the agent. macOS-only (`open -na Ghostty.app --args -e bwoc spawn ...`); non-macOS exits 2 with a hint pointing at the manual `ghostty -e` invocation. Clap-mutex with existing `--tmux`.
- **Dashboard `g` hotkey** — mirrors `t` (tmux chat) but targets Ghostty. Help overlay row added. See [`notes/2026-05-23_chat-ghostty-launcher.md`](notes/2026-05-23_chat-ghostty-launcher.md).

**Cargo SemVer 2.0.0 + auto-version sentinel for minor/major**

- **Workspace version** (`build(version)` `b6885f8`) — `Cargo.toml` workspace.package version `0.1.721` → `2.0.0`. Aligns the Cargo SemVer with the BWOC 2.0 release identity. Per VERSION.md policy: Cargo SemVer captures dev checkpoints (auto-bumped on every edit), CalVer captures release identity.
- **Auto-version hook gains minor/major support** — `.claude/hooks/auto-version.sh` now reads `.bwoc/next-bump.<domain>` sentinel files (one-shot, deleted after consume). Defaults to patch when sentinel is absent. New `scripts/queue-bump.sh <software\|document> <minor\|major\|patch>` helper. See [`notes/2026-05-23_version-2-0-0-and-auto-bump-levels.md`](notes/2026-05-23_version-2-0-0-and-auto-bump-levels.md).

**HITL cleanup pass (4 small fixes from /investigate audit)**

- **`bwoc status --banner`** (`refactor(hitl)` `2e6a754`) — new flag on `bwoc status <agent>` replays the daemon's startup "I am alive" multi-line block from the manifest. No daemon required. Mutex with `--all`. Honors `--lang`. `--banner --json` emits `{"banner": "..."}`. 6 new FTL keys EN+TH; 3 new tests.
- **Dashboard refusal badge** — detail pane now renders `Refused: N` + sub-line `last refused: <reason> from <from>` in yellow when N > 0; omitted when N == 0. New `livecheck::refusal_summary()` helper reads `.bwoc/inbox.refusals.jsonl`.
- **`start`/`stop` non-TTY consistency** — single-agent paths previously failed silently when non-interactive without `--yes`. Now abort with exit 2 + actionable message matching `retire`'s pattern.
- **Stop-hook failure surfacing** — `inbox-auto-reply.sh` now captures stdout/stderr from `bwoc send --reply-to` and appends a diagnostic line to `<self>/.bwoc/agent.log` on non-zero exit. Happy path stays silent.
- See [`notes/2026-05-23_hitl-cleanup-pass.md`](notes/2026-05-23_hitl-cleanup-pass.md).

### Migration from v2026.5.23-1

Existing agents with `gemini` backend need two edits:

```bash
# 1. Rename the symlink in each agent dir (and template if you forked it)
cd agents/<your-agent> && mv GEMINI.md AGY.md
# 2. Edit .bwoc/agents.toml entries:
#    backend = "gemini"   →   backend = "agy"
```

Or regenerate with `bwoc new <name> --force` after the upgrade. Manifests without a `trust` block load unchanged (all fields optional with permissive defaults). Inbox envelopes without `messageId` are still readable (the field is additive — old envelopes pass through unmodified).

## [v2026.5.23-1] — 2026-05-23

### Fixed

- **Release workflow race condition** — five parallel matrix jobs each called `softprops/action-gh-release@v2` with create-or-update semantics; one created the release first, then the next-arriving job raced and failed with "Validation Failed: already_exists". Refactored into one `create-release` job (`gh release create --generate-notes`) + per-target matrix jobs that only `gh release upload --clobber`. `v2026.5.23-1` shipped all 10 assets (5 binaries + 5 sha256) on the first run, no rerun needed.

## [v2026.5.23-0] — 2026-05-23

First public release of the BWOC framework. CalVer scheme: `v<YYYY>.<M>.<D>-<patch>`.

### Added

Everything documented under the prior `[Unreleased]` "Phase 1 v2.0 work in progress" rollup is included in this release. Highlights:

**Open-source project hygiene**

- `VISION.md` + `VISION.th.md` — project purpose, the arc BWOC models, success criteria, non-goals, tradeoff principles. Bilingual (EN canonical, TH translation).
- `SECURITY.md` — coordinated disclosure process; scope; links to the existing threat model.
- `CODE_OF_CONDUCT.md` — BWOC-native (Sīla 5 prohibitions + Brahmavihāra 4 dispositions); explicitly non-sectarian.
- `VERSION.md` — current version mirror, source-of-truth pointer to `Cargo.toml`, SemVer policy, phase-vs-version distinction.
- Root `README.md` Tech Stack section, badges (License · Rust · platforms · languages · status), table of contents, and footer (Contributing · Security · CoC · License).

### Added

**Open-source project hygiene**

- `VISION.md` + `VISION.th.md` — project purpose, the arc BWOC models, success criteria, non-goals, tradeoff principles. Bilingual (EN canonical, TH translation).
- `SECURITY.md` — coordinated disclosure process; scope; links to the existing threat model.
- `CODE_OF_CONDUCT.md` — BWOC-native (Sīla 5 prohibitions + Brahmavihāra 4 dispositions); explicitly non-sectarian.
- `VERSION.md` — current version mirror, source-of-truth pointer to `Cargo.toml`, SemVer policy, phase-vs-version distinction.
- Root `README.md` Tech Stack section, badges (License · Rust · platforms · languages · status), table of contents, and footer (Contributing · Security · CoC · License).

**Specification**

- `PHILOSOPHY.en.md` + `PHILOSOPHY.th.md` §0.1 *"The Arc"* — establishes **uppāda · ṭhiti · vaya** (AN 3.47 Saṅkhata Sutta) as the architectural shape underlying all 22 frameworks.

**Implementation — Phase 1 v2.0 (Rust)**

- Cargo workspace at the repo root: edition 2024, resolver 3, MSRV 1.85.
- `crates/bwoc-core` — shared types; declares `LifecyclePhase { Uppada, Thiti, Vaya }`.
- `crates/bwoc-cli` — `bwoc` binary with `--lang` flag (precedence: `--lang` flag > `BWOC_LANG` env > `$LANG` env > `en` fallback) and clap subcommand surface.
- `crates/bwoc-cli` — **`bwoc check [path]`** implemented. Full feature parity with `modules/agent-template/scripts/check-agent-neutrality.sh`: AGENTS.md existence, backend symlink validation (AGY/CODEX/KIMI → AGENTS.md), CLAUDE.md handling (symlink or standalone), `config.manifest.json` JSON validation, required placeholders, no YAML frontmatter, no wikilinks, no hardcoded model IDs/tool names, no backend-specific phrasing. Read-only; exit 0 = pass, 1 = violations. Pure-data `audit()` + `print_report()` for testability; two unit tests cover wikilink detection and missing-target case.
- `crates/bwoc-cli` — **`bwoc new <name> --role ... --primary-model ... --lint-cmd ... --format-cmd ... --test-cmd ... --build-cmd ...`** implemented. Ports `incarnate.sh` plus the manifest-input spec from `INCARNATION.en.md` §"Setting the Manifest". Recursively copies template (skips `.git/`, `*.example.*`), creates backend symlinks (Unix only; Windows deferred), writes a flat resolved manifest. Kebab-case name validation. Refuses if target exists. Auto-detects template by walking up cwd ancestors. Live end-to-end verified: `bwoc new` then `bwoc check` returns 15 PASS / 0 violations.
- `crates/bwoc-cli` — **`bwoc new` interactive TTY prompts** for missing required fields. Uses `std::io::IsTerminal` (no new dep). On TTY: prompts each missing field with `{key} ({description}): ` where description comes from the template's `config.manifest.json` `requiredConfig.<field>.description`. On non-TTY: collects ALL missing fields in one pass and fails fast with exit code 2 and a comma-separated list — no partial blocking on stdin in CI. Empty prompt response is treated as missing. Two new unit tests cover the fail-fast path and template-description loading.
- `crates/bwoc-cli` — **`bwoc spawn [--path <agent>] [--backend <claude\|agy\|codex\|kimi>] [-- <args>...]`** implemented. Validates the path is a BWOC agent (has `AGENTS.md`), then exec's the backend CLI in the agent's directory via `std::process::Command::status()` (cross-platform; propagates exit code). Default backend is `claude`. Backend-not-found returns actionable "backend CLI 'X' not found on PATH" error. Extra args after `--` pass verbatim to the backend. Four new unit tests cover backend CLI mapping, missing-path rejection, non-agent-dir rejection, and template acceptance. Live verification: `bwoc spawn --path modules/agent-template --backend kimi` successfully launched Kimi Code CLI in the agent directory.

**Phase 1 v2.0 uppāda surface — DoD reached**

The three-command uppāda arc (`bwoc new` → `bwoc check` → `bwoc spawn`) now works end-to-end via the Rust CLI without any shell-script invocation. Software-Version 0.1.21.

- `bwoc-core::workspace::{Workspace, WorkspaceMeta, WorkspaceDefaults, AgentsRegistry, AgentEntry}` — types for `.bwoc/workspace.toml` and `.bwoc/agents.toml` with TOML serde + load/save. New workspace-level dep: `toml = "0.9"`. Three unit tests cover workspace roundtrip, empty agents.toml, and agents-with-entries roundtrip.
- `crates/bwoc-cli` — **`bwoc init [path] [--force]`** implemented. Creates `.bwoc/workspace.toml` (name auto-derived from directory; version `0.1.0`; created stamp UTC ISO 8601) + `.bwoc/agents.toml` (empty registry with a comment header) + the `agents/` directory (per `agents_dir` default). Refuses if `workspace.toml` already exists; `--force` overrides. UTC ISO 8601 stamp computed from `SystemTime` + a small proleptic-Gregorian conversion to avoid pulling in `chrono`/`time`. Four new unit tests cover creation, idempotency refusal, force-overwrite, and date-format anchors (epoch boundaries + 2024 leap day).
- `crates/bwoc-cli` — **`bwoc workspace info [path]`** + **`bwoc workspace validate [path]`** implemented. `info` dumps resolved workspace path, config (name/version/created/defaults), and agent count + per-agent rows from `agents.toml`. `validate` runs the 5 rules from `WORKSPACE.en.md` §"Validation Rules" — `.bwoc/` exists; `workspace.toml` parses + has required `name`/`created` fields; `version` is parseable SemVer (strict X.Y.Z); `agents.toml` parses; `agents_dir` exists — and exits 0 (complete) or 2 (violations). Short-circuits early on structural failures (missing `.bwoc/`, malformed `workspace.toml`). Pure-data `validate()` + `print_validation_report()` for testability; 4 new unit tests cover SemVer validation, missing `.bwoc/`, clean workspace, and bad SemVer. Live-verified against `bwoc init`'d workspace: 7 PASS / 0 violations; degraded scenario (deleted `agents/`) yields 6 PASS / 1 FAIL with the missing-dir message.
- `crates/bwoc-agent` — **real runtime, no longer a stub.** Reads `config.manifest.json` from the current directory and prints structured liveness with the agent identity (`I am alive: <agentId>` + role + model + fallback + memory + version). Exit 0 on success; exit 2 if cwd is not an incarnated agent (missing `config.manifest.json`) with an actionable message; exit 1 on manifest parse failure. Pure-data `liveness_banner(&Manifest) -> String` separated from `main` for unit testability; 2 new unit tests cover required-fields presence and optional-fallback omission. Live-verified inside an incarnated agent directory: prints all six lines correctly; non-agent dir gives "no config.manifest.json in <path>" and exits 2.
- `crates/bwoc-cli` — **`bwoc new` auto-registers the new agent in the enclosing workspace's `.bwoc/agents.toml`** when one is found. Walks ancestors from `target.parent()` for `.bwoc/workspace.toml`; if found, appends an `AgentEntry { id, path (relative to workspace root), backend, incarnated (UTC ISO 8601), status: "active" }` to the registry. New `--backend` flag (defaults `claude`) records which LLM backend the agent runs against. Best-effort: registration failures log a warning but do NOT fail the incarnation (the agent files are already valid on disk). Refuses to register a duplicate agent_id (`NewError::DuplicateRegistration` — user must `bwoc retire` first). Outside any workspace, the report says "No workspace found in ancestors — agent not registered in any agents.toml". 1 new unit test for ancestor-walk. Live-verified both scenarios.
- `crates/bwoc-cli/src/util.rs` — extracted shared `utc_now_iso8601()` + `format_iso8601(secs)` helpers (previously in `init.rs`), now consumed by both `init` and `new`. 1 unit test covers the same 4 epoch-anchor fixtures.
- `crates/bwoc-cli/src/user_home.rs` — Phase 1 minimum `~/.bwoc/` bootstrap per `WORKSPACE.en.md` §"Central Memory". `ensure_initialized()` creates `~/.bwoc/` + an empty `config.toml` (with a header pointing at the spec) if missing; idempotent and cheap when they exist. Cross-platform home-dir lookup via `$HOME` (Unix) / `%USERPROFILE%` (Windows), no `dirs` crate dep. Called from `main` at startup as best-effort — failure logs a warning but does not block commands. Memory/, workspaces.toml, logs/ are deferred to the commands that need them (Mattaññutā — don't create speculatively). 2 unit tests cover creation + idempotency-without-overwrite. Live-verified: `HOME=/tmp/fake-home bwoc` creates `.bwoc/config.toml` from scratch; `env -u HOME bwoc` prints the warning and still runs.
- `crates/bwoc-core` — **`manifest::Manifest`** type with serde camelCase keys (`agentId`, `primaryModel`, `lintCmd`, ...), `load_from_path` + `save_to_path`, `ManifestError` (thiserror) for IO + JSON failures. Two unit tests cover JSON roundtrip and camelCase serialization with `skip_serializing_if` for None options.
- `scripts/install.sh` — one-command install of the `bwoc` CLI (`./scripts/install.sh` runs `cargo install --path crates/bwoc-cli --locked` with toolchain check + PATH hint).
- `crates/bwoc-agent` — minimal "I am alive" runtime stub shipped with each incarnated agent.
- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — Project Fluent locale skeletons; **TH and EN ship at launch**; any future language is a folder drop.

**Crate-level documentation**

- `crates/bwoc-core/README.md` — pure-data scope, `LifecyclePhase` arc surfacing in code.
- `crates/bwoc-cli/README.md` — install, `--lang` precedence example, command surface table organized by arc phase.
- `crates/bwoc-agent/README.md` — phase-scoped responsibility table (Phase 1 = liveness only; Phase 2 = task loop + socket; Phase 3 = interconnect + vaya).

**Framework reference**

- `docs/en/GLOSSARY.en.md` + `docs/th/GLOSSARY.th.md` — single alphabetized lookup table of every Pali term in BWOC with one-line engineering meaning. Bilingual. Designed so non-Buddhist newcomers can read framework code/specs without learning all 22 frameworks first.
- `docs/en/ARCHITECTURE.en.md` + `docs/th/ARCHITECTURE.th.md` — implementation stack (framework → template → agent → CLI → runtime), `bwoc spawn` information flow, backend-neutrality mechanism, multilingual structure across docs / root metadata / CLI locales, and trust boundary table cross-referencing `THREAT-MODEL`. Distinct from the conceptual stack in `PHILOSOPHY` and `README`.
- `docs/en/INCARNATION.en.md` + `docs/th/INCARNATION.th.md` — canonical step-by-step "how to create a new agent" doc consolidating content previously scattered across `incarnate.sh` comments, root README, and `modules/agent-template/README.md`. Covers prerequisites, six-step walkthrough, adding a backend, multilingual setup, verification checklist, and post-incarnation reading path. **Extended with**: "Setting the Manifest" section spec'ing that `bwoc new` accepts manifest fields via flags + interactive TTY prompts (non-TTY = fail-fast), driven by the `requiredConfig` schema in `config.manifest.json`; "Editing the Manifest After Incarnation" specifies direct file edit as canonical with `bwoc manifest set/get` deferred to Phase 2.

**Continuous integration**

- `.github/workflows/ci.yml` — minimal CI on ubuntu-latest: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo build --workspace`, `cargo test --workspace`. Single-OS by intent (multi-OS matrix + release pipeline are Phase 2). Scaffold passes all four gates locally before CI is wired.
- `.github/workflows/docs.yml` — runs the `*.md` naming audit on every PR/push that touches markdown. Three gates per `docs/en/NAMING.en.md` §Audit: (A) root-level files must match `UPPERCASE.md`, `UPPERCASE.<lang>.md`, or the Claude Code convention `CLAUDE.local.md`; (B) files inside `docs/<lang>/` and `modules/agent-template/docs/<lang>/` (mindepth 2) must match `UPPERCASE.<lang>.md`, with slot READMEs exempt; (C) anything under `*/notes/` must match `YYYY-MM-DD_<title>.md`. Emits `::error::` GitHub annotations on violations and exits non-zero. Audit greps refined this iter (allow `.local` suffix at root; `mindepth 2` to skip the docs/ root which holds slot-level examples). `NAMING.en.md` + `NAMING.th.md` + `.claude/skills/check-naming/SKILL.md` updated to keep the documented greps identical to what CI runs.

**Workspace resolution promoted to `workspace info` / `validate`**

- `crates/bwoc-cli/src/workspace.rs` — `run_info` and `run_validate` now use the full WORKSPACE.en.md resolution chain (`find_workspace_root`: explicit path → `BWOC_WORKSPACE` env → ancestor walk → cwd → exit 2). Previously they used cwd-only fallback. Backward compatible — passing an explicit path still works. New behavior: running `bwoc workspace info` or `bwoc workspace validate` from any subdir of a workspace now finds the workspace (no need to cd to root). Non-workspace dirs get the same actionable "no workspace found ... pass a path, set BWOC_WORKSPACE, or run `bwoc init` first" message as `bwoc list`. Dropped the now-unused `resolve_root` helper. Live-verified 4 scenarios: info from subdir, validate from subdir, info from non-workspace dir (exit 2), info with explicit path.

**Phase 1 v2.0 — DoD reached**

`docs/en/ROADMAP.en.md` and `docs/th/ROADMAP.th.md` "Remaining for ship" tables renamed to "Shipped in Phase 1 v2.0" — all 8 spec'd items + 2 follow-on capabilities (runtime-works-anywhere via embedded template; manual major/minor SemVer bumps) now ✓. Stale "Spec'd, not yet implemented" rows in `notes/2026-05-22_phase-1-v20-foundation.md` cleaned up (iters 6, 7, 10, 11 had implemented them; the notes hadn't been refreshed). Only outstanding Phase 1 work: HELD policy items (CODEOWNERS, ISSUE_TEMPLATE/config.yml) and the user's release-tag decision.

**Runtime: works from any directory**

- `crates/bwoc-cli/src/new.rs` — agent template now **embedded into the binary at compile time** via `include_dir!("$CARGO_MANIFEST_DIR/../../modules/agent-template")`. `resolve_template` chain: `--template <path>` → `$BWOC_TEMPLATE` env → ancestor walk for `modules/agent-template/` → `~/.bwoc/template/` cache → **embedded fallback** (extracted to a pid-tagged tmp dir per invocation). Closes the "bwoc new must be run from inside the framework" UX wart.
- `default_target` updated to mirror the resolution: framework-dev path keeps "drop next to template"; everywhere else defaults to `cwd/agent-<name>` (was previously `template.parent().parent()/agent-<name>` which resolved to `/agent-<name>` when template was a tmp dir).
- `crates/bwoc-cli/Cargo.toml` + workspace `Cargo.toml` — add `include_dir = "0.7"` (1 new transitive dep `include_dir_macros`).
- Live-verified by running `bwoc new busaba ...` from `/tmp/learn-workspace-test/` (no framework in ancestors, no `~/.bwoc/template/` cache) → agent created cleanly with AGENTS.md + the four backend symlinks.

**Version bumping**

- `scripts/bump-version.sh <major|minor|patch> [--software|--document|--both]` — manual SemVer bumps for major/minor (patch is still auto-bumped on every Claude Code edit by `.claude/hooks/auto-version.sh`). Computes the new version, writes back to `Cargo.toml` (Software-Version, canonical) and `VERSION.md` (Software-Version mirror + Document-Version), and refreshes the `Last-Updated` UTC ISO 8601 stamp. Edits via shell — not Claude tools — so the auto-version hook doesn't re-fire and bump on top. Smoke-tested across all 3 levels × 3 targets.

**Installer upgrade**

- `scripts/install.sh` — adds `--force` to `cargo install` so re-running the script **upgrades in place** instead of erroring with "already installed". Detects existing install + phrases the message as "Upgrading bwoc in place (was: X.Y.Z)" vs first-install "Installing"; prints the new version after install. Comment header documents the embedded-template behavior + cross-references `bump-version.sh`.

**Fluent string conversion — `bwoc-agent`**

- `crates/bwoc-agent/src/i18n.rs` — new module (duplicated from `bwoc-cli/src/i18n.rs`, intentionally not extracted to bwoc-core yet — see file header). `bundle_for(lang)`, `t`, `t_with`, plus `resolve_lang()` matching bwoc-cli's chain (`BWOC_LANG` → `LANG` → `en`).
- `crates/bwoc-agent/locales/{en,th}/agent.ftl` — 7 keys: 6 liveness lines (alive, role, model, fallback, memory, version) + 1 missing-manifest error.
- `crates/bwoc-agent/Cargo.toml` — adds `fluent-bundle` + `unic-langid` from workspace deps.
- `crates/bwoc-agent/src/main.rs` — `liveness_banner(&Manifest, &FluentBundle)`; missing-manifest error path also localized. Parse-error path stays English.
- TH translation: "I am alive" → "ฉันยังมีชีวิตอยู่"; labels like "role:"/"model:" stay English (programmer-standard technical terms). 4 new i18n unit tests + 3 banner tests (was 2 — now 7 in bwoc-agent).
- Live-verified: from inside an incarnated agent dir, `bwoc-agent` prints EN banner; `BWOC_LANG=th bwoc-agent` prints TH banner.

**Phase 1 v2.0 Fluent conversion — COMPLETE across all 8 CLI/agent surfaces** (init · list · spawn · workspace info · workspace validate · check · new · bwoc-agent).

**Fluent string conversion — `bwoc new`**

- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — 10 new `new-*` keys: report header lines (incarnated agent + target), workspace-registration status (registered with `$path` / not-registered), next-steps header + 4 numbered steps (cd & check, edit AGENTS.md, edit persona, git commit), and the interactive prompt format (`new-prompt-format` with `$key` + `$desc`). TH: "Incarnated agent" → "สร้าง agent"; "Target" → "เป้าหมาย"; "Next steps" → "ขั้นต่อไป"; "ตรวจสอบ neutrality" for the check sub-step, etc.
- `crates/bwoc-cli/src/new.rs` — `run()` / `incarnate()` / `resolve()` / `resolve_one()` / `print_report()` all now take or thread a `&FluentBundle<FluentResource>`. The interactive prompt format uses `new-prompt-format` instead of the hardcoded `"{key} ({desc}): "` template. Symlink lines stay literal (data, not labels). Error path stays English.
- `crates/bwoc-cli/src/main.rs` — `NewArgs::into_runtime(lang)` symmetric with init/list/spawn.
- Mid-iter fixes: missing `use crate::i18n;` import in new.rs (cascaded into 11 errors); two unit tests updated to pass `lang: "en"` in fixture args and `&bundle` into `resolve()`.
- Live-verified EN ("Incarnated agent: agent-alphaen / Target: ... / Next steps: ...") and TH ("สร้าง agent: agent-alphath / เป้าหมาย: ... / ขั้นต่อไป: ..."). 34 tests pass.

**Fluent string conversion — `bwoc check`**

- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — 9 new `check-*` keys: header, target (with `$target`), 3 status labels (PASS/WARN/FAIL), success summary (with `$warnings`) + its tail line, failure summary (with `$violations`+`$warnings`) + its tail line. TH: `PASS`→`ผ่าน`, `WARN`→`เตือน`, `FAIL`→`ไม่ผ่าน`; "Neutrality check passed." → "การตรวจสอบ neutrality ผ่าน".
- `crates/bwoc-cli/src/check.rs` — `print_report()` now takes a `&FluentBundle<FluentResource>` and renders the header/labels/summary through `i18n::t`/`t_with`. `run()` signature changed to `run(target: &Path, lang: &str)` to thread the language. Finding descriptions (~10 rule-specific lines like "AGENTS.md contains {{agentId}}") stay English — translating those would balloon the .ftl by 15-20 keys for marginal benefit.
- `crates/bwoc-cli/src/main.rs` — Check dispatch passes resolved `lang` into `check::run`.
- Live-verified against `modules/agent-template`: EN ("Target: ..." / "PASS  ..." / "0 violations, 0 warning(s) / Neutrality check passed.") and TH ("เป้าหมาย: ..." / "ผ่าน  ..." / "0 ละเมิด, 0 คำเตือน / การตรวจสอบ neutrality ผ่าน"). 34 tests pass.

**Fluent string conversion — `bwoc workspace validate`**

- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — 5 new keys: `validate-header` (with `$path`), `validate-label-pass`, `validate-label-fail`, `validate-summary-success` (with `$passes`), `validate-summary-failure` (with `$passes` + `$violations`). TH: `PASS` → `ผ่าน`, `FAIL` → `ไม่ผ่าน`, summary phrasings translated.
- `crates/bwoc-cli/src/workspace.rs` — `print_validation_report()` now takes the bundle and renders header + per-finding pass/fail prefix + summary through `i18n::t`/`t_with`. `run_validate` builds the bundle from `args.lang`. Finding descriptions (".bwoc/ exists", "workspace.toml parses", etc.) stay in English — translating ~10 rule-specific strings would balloon the .ftl file; deferred unless requested.
- `crates/bwoc-cli/src/main.rs` — `ValidateArgs.lang` plumbed; dispatch passes the resolved lang in.
- Live-verified 3 scenarios: EN happy (`7 pass(es), 0 violation(s) — workspace is complete.`), TH happy (`7 ผ่าน, 0 ละเมิด — workspace ครบถ้วน`), TH degraded with deleted `agents/` (`6 ผ่าน, 1 ละเมิด — แก้ก่อนใช้งาน workspace นี้`, exit 2).

**Fluent string conversion — `bwoc workspace info`**

- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — 9 new keys: `info-header` (with `$path`), 7 `info-label-*` field labels (name/version/created/backend/lang/agents-dir/agents), and `info-agent-row` (with `$id`, `$status`, `$path`).
- `crates/bwoc-cli/src/workspace.rs` — `info()` now takes a `&FluentBundle<FluentResource>` and renders header + each labeled field + per-agent rows through `i18n::t`/`t_with`. `run_info` builds the bundle from `args.lang`. Error path stays English.
- `crates/bwoc-cli/src/main.rs` — `InfoArgs` now carries `lang`; dispatch passes the resolved `lang` in.
- **Known cosmetic** (carried over from iter 18): the labels were originally hardcoded literals, so the fixed-position colon alignment worked. Now labels vary by language (`name` vs `ชื่อ`, `version` vs `เวอร์ชัน`) and have different byte widths, so alignment is uneven. Acceptable for readability; a proper fix needs Unicode-width-aware padding (`unicode-width` crate or similar).
- Live-verified EN ("Workspace: /tmp/infoi18n / name: infoi18n / version: 0.1.0 / ...") and TH ("Workspace: /tmp/infoi18n / ชื่อ: infoi18n / เวอร์ชัน: 0.1.0 / สร้างเมื่อ: ... / agent: 1").

**Fluent string conversion — `bwoc spawn`**

- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — 1 new `spawn-exec-status` message key with `$backend` and `$path` args. TH uses Thai preposition `ใน` ("in").
- `crates/bwoc-cli/src/spawn.rs` — `spawn()` builds its own bundle and emits the exec-status line via `i18n::t_with`. Error path (BackendNotFound, PathMissing, NotAnAgent, Io) stays English.
- `crates/bwoc-cli/src/main.rs` — `SpawnArgs::into_runtime(lang)` symmetric with init + list.
- Live-verified by spawning the real `codex` CLI in `modules/agent-template` from both EN and TH locales; status line correctly interpolates backend name + path.

**Fluent string conversion — `bwoc list`**

- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — 5 new `list-*` message keys: `list-empty` (with `$path` arg), `list-col-id`, `list-col-status`, `list-col-backend`, `list-col-path`. TH translates `STATUS` → `สถานะ`; the other column labels stay as English ASCII (`ID`/`Backend`/`Path`) since they're programmer-standard terms.
- `crates/bwoc-cli/src/workspace.rs` — `run_list` now drives the success path through `i18n::t` / `t_with`. Error path stays English (same rule as `init`).
- `crates/bwoc-cli/src/main.rs` — `ListArgs` threads `lang` to runtime via `into_runtime(lang)`. Symmetric with `InitArgs`.
- **Known cosmetic**: Rust's `{:<10}` pads by byte count not visual width, so the Thai `สถานะ` column header is slightly off-alignment. Acceptable for now; fixing would require pulling in `unicode-width` and a width-aware formatter (deferred — not blocking readability).
- Live-verified 4 scenarios: EN empty, TH empty, EN populated, TH populated.

**Fluent string conversion — `bwoc init`**

- `crates/bwoc-cli/src/i18n.rs` — added `t_with(bundle, key, &[(name, value)])` for named-arg interpolation. The slice-of-tuples shape keeps call sites ergonomic without exposing `FluentArgs` directly. 1 new unit test (`t_with_interpolates_named_args`).
- `crates/bwoc-cli/locales/{en,th}/cli.ftl` — added 7 `init-*` message keys (success title, three created-file lines, next-steps header, two next-step suggestions). **Fluent gotcha caught**: `.` is not allowed in identifier names, so keys use `init-success-title` style, not `init.success-title`. First attempt panicked at runtime ("ExpectedToken('=')"); fixed by renaming and updating callers.
- `crates/bwoc-cli/src/init.rs` — `run()` now drives the success-path output through `i18n::t` / `t_with` with `lang` threaded down via `InitArgs`. Error path remains in English (`thiserror` localization deferred).
- `crates/bwoc-cli/src/main.rs` — passes the resolved `lang` into `init::InitArgs`.
- **Known cosmetic regression**: Fluent strips leading whitespace from single-line message values, so the `"  + "` indentation in the pre-Fluent `bwoc init` output is gone (output still reads cleanly). Restorable with Fluent's `{""}` empty-string placeable trick when we touch this surface again.

**`--lang` → Project Fluent wiring**

- `crates/bwoc-cli/src/i18n.rs` — new module exposing `bundle_for(lang)` and `t(bundle, key)`. Locale files (`locales/<lang>/cli.ftl`) embedded into the binary at compile time via `include_str!`, so distributed `bwoc` doesn't need to find them on disk. Unsupported language codes fall back to `en`. Fluent's default Unicode bidirectional isolation marks disabled for clean terminal output. Missing-key lookups return a visible `«missing key: <key>»` marker rather than panicking — surfaces gaps during dev. 4 new unit tests (EN content, TH content, unknown-lang fallback, missing-key marker).
- `crates/bwoc-cli/Cargo.toml` — new deps `fluent-bundle` + `unic-langid` (both already in `[workspace.dependencies]` from iter 1's scaffold; just inheriting them now).
- `crates/bwoc-cli/locales/en/cli.ftl` + `locales/th/cli.ftl` — added `default-help-hint` message (EN: "try `bwoc --help`"; TH: "ลองใช้ `bwoc --help`").
- `crates/bwoc-cli/src/main.rs` — replaces the default-no-subcommand `println!` literal with `i18n::t(&bundle, "default-help-hint")` driven by the resolved `--lang`. **This iter wires infrastructure plus ONE message as proof; converting the remaining `println!` literals across `check`/`new`/`spawn`/`init`/`workspace`/`list`/`bwoc-agent` is a follow-up so we don't bundle all string conversions into one iter (Mattaññutā).** Live-verified: `bwoc` → EN; `bwoc --lang th` → Thai; `BWOC_LANG=th bwoc` → Thai; `bwoc --lang ja` → EN fallback.

**`bwoc list`**

- `crates/bwoc-cli` — **`bwoc list [--workspace <path>]`** implemented. Reads the enclosing workspace's `.bwoc/agents.toml` and prints an id/status/backend/path table. Workspace resolution per `WORKSPACE.en.md` §"Workspace Resolution": explicit `--workspace` → `BWOC_WORKSPACE` env → ancestor walk for `.bwoc/workspace.toml` → cwd self-check → fail with actionable exit-2 error. Empty registry prints `(no agents in workspace <path>)` and exits 0. 1 new unit test for ancestor-walk. Live-verified 4 scenarios: empty workspace, two-agent workspace via `--workspace`, ancestor walk from a workspace subdir, and non-workspace dir (exit 2 with actionable message). Same full-resolution logic should later be promoted to `workspace info` / `validate` (logged as follow-up).

**Issue and PR templates (non-policy)**

- `.github/ISSUE_TEMPLATE/bug_report.md` — structured form with BWOC-specific fields: BWOC version, OS, Rust toolchain, backend (claude/agy/codex/kimi), surface (framework/template/CLI/runtime/hooks), and **arc phase** (uppāda/ṭhiti/vaya — where in the agent's life did this surface?). Includes a SECURITY redirect for exploitable defects.
- `.github/ISSUE_TEMPLATE/feature_request.md` — Problem/Solution/Alternatives shape grounded in Ariyasacca 4 (Dukkha → propose; Samudaya implied; Nirodha/Magga in scope section). Optional but-encouraged "Buddhist framework alignment" field referencing GLOSSARY.
- `.github/PULL_REQUEST_TEMPLATE.md` — Summary + What/Related/Checklist/Risk-and-rollback. The Checklist mirrors `CONTRIBUTING.md` §Pull Request Checklist verbatim PLUS adds bilingual-parity + naming-audit + manifest-schema gates that the CI workflows enforce.

These three are explicitly **non-policy** (mechanical forms that mirror existing CONTRIBUTING.md content). The policy-bearing items still HELD: `CODEOWNERS` (review-duty assignment) and `ISSUE_TEMPLATE/config.yml` (contact-routing URLs).

**Implementation logs (new convention)**

- `notes/` directory established with `notes/2026-05-22_phase-1-v20-foundation.md` as the starter — single session covering open-source hygiene + bilingual spec layer + Rust scaffold + auto-versioning + CI + over-engineering protection. Captures decisions, alternatives, and bugs surfaced.
- `CLAUDE.md` — "Implementation Logs (HARD RULE)" section added: every significant change gets `notes/YYYY-MM-DD_<title>.md` per the pattern in `NAMING.en.md`. One note per session, not per file.

**Modules layer (filled previously-empty placeholders)**

- `modules/README.md` — top-level modules overview (`agent-template/` ready · `plugins/` planned · `skills/` planned · `cli/` deprecated). Adds "Adding a new module" guidance.
- `modules/plugins/README.md` — planned framework plugins spec. Defines what plugins are (Tier 2 memory backends, additional LLM-backend integrations, workflow integrations), what they are NOT (vendor-specific shortcuts), and that the loading mechanism lands with the first plugin.
- `modules/skills/README.md` — planned framework skills spec. Distinguishes framework skills from agent skills (per-agent slot) and from `.claude/skills/` (Claude Code project skills).
- `modules/agent-template/mindsets/SPEC.md` — agent slot spec. Mindsets = decision-making frameworks; one mindset per file; Obsidian frontmatter; "When NOT to apply" required; each anchors one Pali principle.
- `modules/agent-template/skills/SPEC.md` — agent slot spec. Skills = concrete capabilities; bounded; verifiable; cross-linked from `interconnect/capabilities.md`; maturity levels L1–L7 per Ariya-dhana 7.

**Tooling and process (Claude Code)**

- `CLAUDE.md` — framework-level guidance for Claude Code sessions.
- `.claude/skills/` — `/incarnate`, `/check-neutrality`, `/check-bilingual`, `/task-log`, `/check-naming` (project-scoped slash commands).
- `.claude/hooks/bilingual-reminder.sh` — `PostToolUse` `Write|Edit` hook reminding to update the matching TH file when an EN doc changes. **Extended** to cover (a) the **reverse direction** for `docs/<lang>/` (editing TH reminds about EN canonical) and (b) **root-level `FILENAME.md` ↔ `FILENAME.th.md`** (e.g., `VISION.md` ↔ `VISION.th.md`). Root-level canonical→translation only fires if the translation already exists, to avoid noisy reminders for unpaired files like `CHANGELOG.md`. Out-of-repo paths exit silently (matches `auto-version.sh` scoping). Pipe-tested all 8 scenarios.
- `.claude/hooks/auto-version.sh` — `PostToolUse` `Write|Edit` hook that auto-bumps SemVer PATCH on every Claude Code edit. Software domain (`.rs` / `.toml` / `crates/*`) bumps `Cargo.toml` `[workspace.package].version`; document domain (`.md`) bumps `VERSION.md` `Document-Version`. Both stamp `Last-Updated` (UTC, ISO 8601). Self-managed files are guarded against self-trigger.
- `docs/en/WORKSPACE.en.md` + `docs/th/WORKSPACE.th.md` — workspace concept spec. Defines on-disk structure (`.bwoc/workspace.toml`, `.bwoc/agents.toml`), validation rules ("complete before work"), CLI surface (`bwoc init`, `bwoc workspace info/validate`), workspace resolution precedence (`--workspace` flag → `BWOC_WORKSPACE` env → ancestor walk → cwd → refuse), central per-user memory at `~/.bwoc/` (config, memory, workspaces registry, logs), and memory scope hierarchy (per-agent → per-workspace → per-user → Tier 2).
- `docs/en/NAMING.en.md` + `docs/th/NAMING.th.md` — unified `*.md` naming standard with 12 categories, rule definitions, quick decision tree, and audit grep snippets. New note pattern `YYYY-MM-DD_<title>.md` (ISO 8601 date prefix + underscore + kebab-case title) for chronological notes; valid locations are `<repo>/notes/`, `<workspace>/.bwoc/notes/`, or `~/.bwoc/notes/`.
- `docs/en/ROADMAP.en.md` + `docs/th/ROADMAP.th.md` — phase-by-phase plan (Phase 1 v2.0 uppāda → Phase 4 fleet). Each phase has Definition of Done and links the spec doc each remaining item refers to. README Status table now points here for the full plan.
- `docs/en/FAQ.en.md` + `docs/th/FAQ.th.md` — newcomer FAQ across Conceptual, Project Mechanics, Setup, Multi-Language and Multi-Backend, Conventions, Operations, and Contributing categories. Extracts the three READMEs Qs and expands with Qs surfaced by VISION/GLOSSARY/ARCHITECTURE/INCARNATION/WORKSPACE/NAMING. README FAQ section now points here for the full set.
- `.claude/settings.json` — registers both hooks for the team.

**Phase 2 + 3 implementation arc** (theme-grouped; per-commit detail in `git log` and [`notes/2026-05-22_phase-2-thiti-surface.md`](notes/2026-05-22_phase-2-thiti-surface.md))

- **Lifecycle verbs** (Phase 3 vaya + state machine):
  - `bwoc retire <name>` (registry removal; `--keep-files` retains agent dir)
  - `bwoc stop <name>` — 3-step escalation ladder: socket `STOP` → SIGTERM → SIGKILL (~3s wait between steps); reports which step ended the daemon
  - `bwoc start <name>` — flips registry status AND spawns `bwoc-agent --serve`; `--no-daemon` opt-out; idempotent across all (status × daemon) combinations
  - `bwoc workspace prune` — reconciles phantom registry entries vs orphan agent dirs; `--apply` removes safe drift

- **Daemon + IPC** (Phase 2 ṭhiti):
  - `bwoc-agent --serve` Unix daemon: writes `.bwoc/agent.{pid,sock}`; line-text IPC protocol (`PING`/`STATUS`/`STOP`) debuggable with `nc -U`
  - Persistent inbox cursor (`.bwoc/inbox.cursor`) — daemon resumes after restart
  - `bwoc ping <agent>` — CLI client for PING
  - Stderr redirect to `<agent>/.bwoc/agent.log` for `bwoc log` to tail
  - `bwoc-agent --version` / `-V` / `--help` / `-h` flags (was: `--serve` only)
  - Windows: `--serve` is a clean cfg-gated stub (default mode + `--version`/`--help` work); named-pipe daemon path queued

- **Messaging stack** (sammā-vācā Phase 0):
  - `bwoc send <agent> <msg>` — JSONL envelope to `<agent>/.bwoc/inbox.jsonl`
  - `bwoc inbox <agent>` — `--limit` · `--json` · `--watch` · `--clear`
  - INBOX column in `bwoc list`
  - Daemon-side inbox watch: announces new envelopes to stderr as they arrive

- **Observation + UX**:
  - `bwoc list` — runtime ●/○ indicator; filters `--status` / `--backend` / `--running`
  - `bwoc status [name]` — health + identity + uptime; per-agent detail surfaces persona scope + mindset/skill/memory counts; `--json` mirrors the human shape
  - `bwoc dashboard` (TUI) — ratatui-based; agents pane + detail pane + 2s auto-refresh + `t` hotkey to spawn chat in a new tmux window + workspace-level projects/notes/memory counts in banner
  - `bwoc chat <agent>` — auto-resolves backend from registry; `--tmux` for new-window mode
  - `bwoc doctor` — env + workspace diagnostic; `--auto` sweeps stale `agent.pid` / `agent.sock` / `inbox.cursor`
  - `bwoc log <agent>` — tails daemon stderr; `-f` follow · `-n N` lines · `--clear` truncate-in-place
  - `bwoc completion <shell>` — bash/zsh/fish/powershell/elvish via clap_complete
  - `bwoc help` — 10 topical guides: `getting-started`, `backends`, `workspace`, `manifest`, `arc`, `lifecycle`, `daemon`, `messaging`, `persona`, `memory`
  - `--json` across read-only commands: `list`, `status`, `workspace info`, `workspace validate`, `check`, `inbox`, `memory list|search`
  - Banner ANSI Shadow wordmark + command index for the no-subcommand case
  - Unicode-width column padding in `bwoc list` (Thai header alignment)

- **Per-workspace memory** (`<workspace>/.bwoc/memory/`):
  - `bwoc init` scaffolds the directory with a README documenting the 4-tier scope hierarchy
  - `bwoc memory list | show | put | search` — full read/write/search CLI with path-traversal refusal, atomic write (stage-to-temp + rename), `--force` overwrite gate, case-insensitive substring search; both human and `--json` output where useful

- **Persona configuration at incarnation**:
  - `bwoc new --scope` / `--out-of-scope` — fill `{{scopeDescription}}` / `{{outOfScope}}` placeholders in AGENTS.md + persona/README.md
  - `bwoc new --mindsets a,b,c` / `--skills a,b,c` — seed stub `.md` files matching the SPEC.md scaffold
  - Manifest schema gained `scopeDescription` + `outOfScope` fields (optional)
  - IncarnationReport surfaces persona_filled + mindset_stubs + skill_stubs counts

- **CI + Release**:
  - `.github/workflows/ci.yml` — matrix build + test across `ubuntu-latest` · `macos-latest` · `windows-latest`; fmt + clippy gated on Ubuntu only (rules are OS-independent)
  - `.github/workflows/release.yml` — triggers on CalVer tag `v<YYYY>.<M>.<D>-<patch>`; 5-target release matrix (Linux x64 + Linux ARM64 + macOS Apple Silicon + macOS Intel + Windows x64); auto-creates GitHub Release with notes + SHA-256 sidecars; `fail_on_unmatched_files: true` so partial releases never ship
  - `.github/workflows/docs.yml` — naming-audit `notes/README.md` exemption added (category 5 slot READMEs)
  - `docs/en/RELEASING.en.md` + `docs/th/RELEASING.th.md` (bilingual pair) — pre-flight, tag-and-push, prerelease vs stable, rollback policy
  - `VERSION.md` "Dual Namespaces" — Cargo SemVer (auto-bumped per edit, dev checkpoint) + Release CalVer (public release identity, manual tag)

- **Refactor + hygiene**:
  - `crate::livecheck` module consolidates 5 byte-identical copies of `signal_zero_alive` / `running_pid` / `query_uptime` / `format_uptime` / `inbox_count` across status/doctor/workspace/dashboard/start
  - End-to-end smoke test at `crates/bwoc-cli/tests/smoke.rs` — `init → new → list` against a real tempdir
  - Test-friendly `cfg(unix)` gating on signal-0 / HOME-env / `/tmp`-path tests for Windows portability
  - `bwoc-agent` Windows stub: `serve_loop` + 4 helpers cfg-gated; non-Unix returns "daemon is Unix-only" exit 2

- **Docs sync**:
  - ROADMAP + README + VERSION.md + CLAUDE.md all kept current with shipped features; multiple per-iter sync commits
  - Root-level bilingual policy documented in CLAUDE.md (which docs require TH pair, which don't)
  - CHANGELOG Known Issues trimmed from 4 → 1 stale items removed
  - 4 implementation notes under `notes/`: bwoc-new UX, gap-analysis, Pages+release pipeline, Phase 2 ṭhiti surface backfill

**Late Phase 2 polish** (since the bullet block above)

- **Memory CRUD completed**:
  - `bwoc memory put <name> [--file <p>] [--force]` — write from stdin or file; atomic stage+rename
  - `bwoc memory search <query> [--json]` — case-insensitive substring across entries
  - `bwoc memory rm <name> [--yes]` — delete an entry (TTY confirm; refuses README.md and traversal)
  - `bwoc memory show --all [--json]` — print every entry concatenated with `# === <name> ===` headers (or JSON array); pairs with agent-boot context loading
  - `bwoc help memory` — topic doc covering all 4 CRUD verbs + search

- **Dashboard hotkey triad**:
  - `t` opens `bwoc spawn` in a new tmux window (chat — original)
  - `l` opens `bwoc log -f` in a new tmux window (daemon log live tail) — NEW
  - `i` opens `bwoc inbox --watch` in a new tmux window (inbox live tail) — NEW
  - Window naming `<agent-id>` / `<agent-id>-log` / `<agent-id>-inbox` so all three can coexist

- **`bwoc list` filter + ordering surface**:
  - `--inbox-pending` — filter to agents with unread envelopes
  - `--sort id | inbox | incarnated | backend` — stable sort with informative default
  - `--count` — emit just the row count (integer or `{"count": N}` with `--json`); short-circuits after filter+sort for shell-script idioms

- **`bwoc doctor`**:
  - WARN on oversized `agent.log` (10 MiB threshold; `--auto` truncates — diagnostic chatter)
  - WARN-only on oversized `inbox.jsonl` (5 MiB threshold; `--auto` explicitly refuses to discard user data — Sammā-vācā)
  - `--json` output with `{ results, summary, exit }` stable shape for CI gating
  - `bwoc help doctor` topic — full status taxonomy, all 7 checks, deliberate asymmetry on user-data handling

- **Workspace surfaces**:
  - `bwoc workspace info` text + JSON gained per-workspace `Resources` block (projects / notes / memory counts)
  - Dashboard banner shows the same counts

- **bwoc-agent**:
  - `--version` / `-V` / `--help` / `-h` flags (was: only `--serve` handled)

**Mass-action verb matrix + shell ergonomics** (latest batch)

- **Six verbs gain `--all`** for fleet-wide operations:
  - `bwoc stop --all` — signal-escalation per agent (STOP → SIGTERM → SIGKILL)
  - `bwoc start --all` — flip registry + spawn daemons (`--no-daemon` opt-out)
  - `bwoc status --all` — full detail block per agent (loop of single-agent view)
  - `bwoc check --all` — fleet-wide neutrality audit with `{ agents[], summary }` JSON
  - `bwoc ping --all` — mass liveness probe (not-running labeled but not failed)
  - (`bwoc list` is already always all-agents; `bwoc retire --all` deliberately omitted — destructive)
  - Each uses clap `ArgGroup` for the `name`/`--all` mutex; trying neither or both → parse error

- **Script-friendly read flags**:
  - `bwoc list --count` / `--names-only` — integer or bare ids for shell loops
  - `bwoc memory list --count` / `--names-only` — same on memory entries
  - `bwoc inbox <name> --count` — envelope count for `if [ $(...) -gt 0 ]`
  - `bwoc workspace info --path-only` — for `cd "$(bwoc workspace info --path-only)"`

- **List filters + sort**:
  - `--inbox-pending` (agents with unread envelopes), combinable with --running/--status/--backend
  - `--sort id | inbox | incarnated | backend` (stable; default = registry order)

- **`bwoc memory put` write modes**:
  - 3 sources: inline positional `[content]` > `--file <path>` > stdin
  - 3 write modes: create (default) / `--force` overwrite / `--append`
  - All atomic via .tmp staging + rename

- **`bwoc send`**: inline `<msg>` OR `--file <path>` (clap mutex; same UX as memory put)

- **Workspace attention summary** — `bwoc workspace info` + dashboard banner show
  total pending inbox count across all agents when > 0; cross-link to
  `bwoc list --inbox-pending` for the "what needs attention?" workflow.

- **`bwoc help` topics 10 → 11**: + `doctor` (status taxonomy + auto-fix policy)

**Process supervision + remaining UX polish** (most recent batch)

- **`bwoc supervise <agent>`** — restart-on-crash supervisor closes a
  Phase 2 "Remaining for ship" item. Spawns `bwoc-agent --serve`,
  waits, respawns on non-zero exit; rate-limited (default 10/min,
  `--max-restarts-per-min N`). Clean exit (status 0) stops the
  supervisor. SIGINT/SIGTERM via ctrlc → exit 0. Stderr → same
  `agent.log` as `bwoc start`, so `bwoc log -f` works against
  supervised daemons. Usage: `tmux new-window 'bwoc supervise alpha'`
  or inside the user's own systemd unit. New `ctrlc` dep on bwoc-cli
  (already a workspace dep for bwoc-agent).

- **`bwoc retire --keep-memory`** — third file mode between default
  (delete) and `--keep-files` (retain all). Removes everything under
  the agent dir EXCEPT `memories/`, preserving accumulated knowledge
  for future agents. clap mutex with `--keep-files`.

- **`bwoc inbox --all`** — print every agent's inbox concatenated,
  each preceded by a `=== <agent-id> (N message(s)) ===` header.
  Empty inboxes still get a header. `--clear` and `--watch` are
  refused with `--all` (mass-clear too destructive; mass-watch
  interleaves confusingly). JSON shape: `{ agents: [{ agent, total,
  shown, messages }] }`.

- **UPTIME column on every overview surface** — `bwoc list` (table)
  and `bwoc status` (table) gained UPTIME between BACKEND and INBOX/
  MODEL. `bwoc list --json` and `bwoc status --json` gained
  `running` + `uptime_seconds` (nullable). All four surfaces share
  the same `livecheck::query_uptime` + `format_uptime` data path.

- **`bwoc check --all`** — fleet-wide neutrality audit. Iterates the
  workspace registry, runs `audit()` per agent, prints per-agent
  sections + fleet summary. JSON shape: `{ workspace, agents[],
  summary }`. Exit 1 iff any agent has violations.

- **`bwoc ping --all`** — mass liveness probe across the workspace.
  Agents with no live socket get "not running" label (not a
  failure; they're just stopped). Protocol drift / connection errors
  → exit 1.

- **Memory write/sort ergonomics**:
  - `bwoc memory put <name> "inline"` — third source mode (precedence:
    inline > --file > stdin); trailing newline appended automatically
  - `bwoc memory put <name> --append` — accumulate to existing entry
    (read-modify-write staged atomically; clap mutex with `--force`)
  - `bwoc memory list --json` adds inline `count` + `total_bytes`
    aggregates
  - `bwoc memory list --sort name|size|modified` — mirror of
    `bwoc list --sort` for memory entries

- **`bwoc send <agent> --file <path>`** — second message source
  (clap mutex with inline `<msg>`); trailing newlines trimmed so
  vim/EOF newline doesn't bloat the envelope.

- **`bwoc help` topic 11 → 12**: + `script` (shell idioms for
  --count / --names-only / --json / --path-only across all read
  commands)

**Write-command JSON family + dashboard help + memory sort** (most recent)

- **JSON-everywhere completed across write commands**:
  - `bwoc new --json` — incarnation report `{ agent_id, target,
    registered_in, symlinks, mindset_stubs, skill_stubs, persona_filled }`
  - `bwoc start --json` (requires `--yes`) — `{ workspace, agent,
    daemon_spawned, daemon_pid, already_running, registry_updated }`
  - `bwoc stop --json` (requires `--yes`) — `{ workspace, agent,
    daemon_outcome, registry_updated }` (outcome: not_running /
    socket_ok / sigterm / sigkill / could_not_kill)
  - `bwoc retire --json` (requires `--yes`) — `{ workspace, agent,
    path, mode, registry_updated }` (mode: delete / keep_files /
    keep_memory)
  - `bwoc workspace prune --json` — `{ workspace, phantoms, orphans,
    applied, removed }` for CI gating
  - `bwoc supervise --json` — emits one structured event per action
    (watch_start / spawn / crash_respawn / clean_exit / rate_limit_hit /
    signal_stop / spawn_failed)
  - `bwoc inbox --watch --json` (was rejection, now streams) — one
    compact JSON envelope per line for log shippers
  - Safety guard on destructive verbs: --json requires --yes
    (scripted destructive ops without explicit ack → exit 2)

- **Dashboard `?` overlay** — centered help popup listing every
  hotkey, dismissed on any key. Footer gains a `?: help` chip.

- **`bwoc memory list --sort name|size|modified`** — mirror of
  `bwoc list --sort`. Default = name (alphabetical). Unknown field
  → exit 2 with accepted-values hint. Entry mtime captured via
  `metadata().modified()`.

- **`bwoc memory list --json` aggregates** — inline `count` +
  `total_bytes` fields so CI doesn't have to walk entries[] to
  compute totals.

- **`bwoc help --all`** — concatenated all-topics output with
  `# === <name> ===` Markdown-safe separators for offline reading
  or pipe into docs generator.

### Changed

- `modules/agent-template/README.md` — added badges, table of contents, and footer; trimmed the "Incarnating a New Agent" section to a quickstart that points at `docs/en/INCARNATION.en.md`.
- `README.md` "Getting Started > As an Agent Author" — replaced outdated manual `cp -r` recipe with the canonical `./scripts/incarnate.sh` invocation and link to `INCARNATION.en.md`.
- `README.md` FAQ — trimmed to top-3 + link to full `docs/en/FAQ.en.md`.
- `README.md` Status — trimmed to a summary table + link to `docs/en/ROADMAP.en.md` for the full phase plan.
- `VERSION.md` — restructured header to expose `Software-Version`, `Document-Version`, `Last-Updated` (UTC ISO 8601). Auto-managed by `.claude/hooks/auto-version.sh`.
- `crates/bwoc-cli/README.md` — added workspace command surface (`bwoc init`, `bwoc workspace info/validate`) and `--workspace` flag declaration.
- `modules/agent-template/conventions.md` — pointer to `docs/en/NAMING.en.md` as the comprehensive `*.md` naming standard; softened validation-checklist rule from "File names are kebab-case.md" to "Markdown file names follow NAMING.en.md (12 categories)"; renamed "Files & Directories" section to "Directories" since file naming now lives in NAMING.
- `modules/agent-template/docs/th/PHILOSOPHY.th.md` — corrected `## ๑. หลักธรรมหลัก ๑๔ ประการ` to `## ๑. หลักธรรมหลัก ๒๒ ประการ` to match the EN side (22 verified by counting groups A–F).
- `.claude/hooks/auto-version.sh` — two silent bugs fixed: (1) GNU-only sed `0,/regex/s||...|` replaced with portable `s|^version = "X.Y.Z"$|version = "X.Y.Z"|` for Cargo.toml bumps on macOS BSD sed; (2) out-of-repo file paths (e.g., `~/.claude/projects/.../memory/*.md` edits) no longer trigger Document-Version bumps — added early-exit when the file is not under the workspace root. Both verified via pipe-test.
- `modules/agent-template/AGENTS.md` reference set — unchanged (the v2.0 spec is the baseline this Phase implements).

### Deprecated

- `modules/cli/` — replaced by `crates/bwoc-cli/`. A stub README is left in place; the directory will be removed once nothing references it.

### Conventions

- **Root-level bilingual files**: `FILENAME.md` is the English canonical; `FILENAME.<lang>.md` is a translation (e.g. `VISION.md` ↔ `VISION.th.md`). Parallel to but distinct from the `docs/<lang>/` pattern used inside the agent template.

### Known Issues

- Two `CONTRIBUTING.md`-referenced policy files are HELD pending user direction: `.github/CODEOWNERS` (review-duty assignments) and `.github/ISSUE_TEMPLATE/config.yml` (Discussions URL + contact routing). The non-policy issue/PR templates (`bug_report.md`, `feature_request.md`, `PULL_REQUEST_TEMPLATE.md`) shipped earlier. See `.claude/loop-roadmap.md` for the HELD status detail.

---

## Pre-Phase-1

Framework specification existed prior to this changelog: `AGENTS.md` v2.0, the 22 Buddhist-framework mappings in `PHILOSOPHY.en.md`, the PRD (Ariyasacca 4), SRS (Magga 8), lifecycle, threat model (Taṇhā 3 + Sīla 5), and self-improvement (Paññā 3) documents.
