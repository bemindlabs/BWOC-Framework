# Changelog

All notable changes to BWOC are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/). See [`VERSION.md`](VERSION.md) for the current version and phase status.

## [Unreleased]

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
