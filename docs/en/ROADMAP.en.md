---
title: Roadmap
parent: English
nav_order: 6
---

# Roadmap

Phase-by-phase plan for BWOC. **Phases** describe implementation milestones; each may span several SemVer releases. See [`VERSION.md`](../../VERSION.md) for the version-vs-phase distinction. See [`VISION.md`](../../VISION.md) for success criteria at 1-year and 3-year horizons.

---

## Current Status

**Active phase:** Phase 3 — *vaya + interconnect* — **DoD met** (interconnect routing + worktree lifecycle + `bwoc retire` full vaya all shipped 2026-05-23). Remaining Phase 3 items (Trust v2, Tier 2 memory) are deferred, off the DoD. Phase 1 v2.0 and Phase 2 DoDs also met. **BWOC 2.0** released as `v2026.5.23-2`.
**Software-Version:** see [`VERSION.md`](../../VERSION.md).
**Document-Version:** see [`VERSION.md`](../../VERSION.md).

---

## Phase 1 v2.0 — uppāda Foundation

**Definition of done:** end-to-end **uppāda** for one backend — incarnate · check · spawn an agent that runs.

### Completed

- Cargo workspace (`bwoc-core`, `bwoc-cli`, `bwoc-agent`) scaffold; edition 2024; MSRV 1.85.
- `VERSION.md` with `Software-Version`, `Document-Version`, and `Last-Updated`; auto-managed by `.claude/hooks/auto-version.sh`.
- Open-source hygiene: `VISION.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`; root `README.md` with badges, TOC, footer.
- Spec docs (all bilingual EN/TH): `PHILOSOPHY` §0.1 *The Arc*, `GLOSSARY`, `ARCHITECTURE`, `INCARNATION`, `WORKSPACE`, `NAMING`.
- Crate READMEs (`bwoc-core`, `bwoc-cli`, `bwoc-agent`).
- Claude Code tooling: 4 project skills (`/incarnate`, `/check-neutrality`, `/check-bilingual`, `/task-log`); 2 PostToolUse hooks (`bilingual-reminder`, `auto-version`).
- `incarnate.sh` and `check-agent-neutrality.sh` shell scripts in the template (work today; will be ported to Rust).

### Shipped in Phase 1 v2.0

All items below are now implemented. The phase's Definition of Done (end-to-end **uppāda** for one backend) is **met**. Only HELD policy items (`CODEOWNERS` · `ISSUE_TEMPLATE/config.yml`) remain pending user direction; the release pipeline now exists (see Phase 2).

| Item | Spec | Status |
|---|---|---|
| `bwoc init [path]` | [`WORKSPACE.en.md`](WORKSPACE.en.md#cli-surface) | ✓ |
| `bwoc workspace info` · `validate` | [`WORKSPACE.en.md`](WORKSPACE.en.md#cli-surface) | ✓ |
| `bwoc new <name>` (port of `incarnate.sh`) | [`INCARNATION.en.md`](INCARNATION.en.md) | ✓ |
| `bwoc check [path]` (port of `check-agent-neutrality.sh`) | [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) | ✓ |
| `bwoc spawn <name>` (minimal `exec`) | [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md#information-flow--bwoc-spawn-agent-foo) | ✓ |
| `bwoc list` (reads `.bwoc/agents.toml`) | [`WORKSPACE.en.md`](WORKSPACE.en.md) | ✓ |
| `--lang` flag wired to Project Fluent (TH + EN locales) | [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) | ✓ all 8 surfaces (init/list/spawn/workspace info/workspace validate/check/new/bwoc-agent) |
| `/check-naming` skill (audit `*.md` against `NAMING.en.md`) | [`NAMING.en.md`](NAMING.en.md#audit) | ✓ + wired into `.github/workflows/docs.yml` |
| Runtime works from any directory | embedded `include_dir!` agent template + `BWOC_TEMPLATE` env + `~/.bwoc/template/` cache | ✓ |
| Manual major/minor version bumps | `scripts/bump-version.sh <level> [--software\|--document\|--both]` | ✓ (patch still auto-bumped by hook) |

---

## Phase 2 — ṭhiti Operations

**Definition of done:** an agent operates with a real control surface; multiple backends are exercised; releases are reproducible.

### Shipped in Phase 2

| Item | Notes |
|---|---|
| `bwoc-agent --serve` daemon | Unix-only (`.bwoc/agent.pid` + `.bwoc/agent.sock`; cfg-gated stub on Windows) |
| IPC control socket — line-text protocol | `PING`/`STATUS`/`STOP` over Unix domain socket; debuggable with `nc -U` |
| `bwoc status [name]` | Per-agent health + runtime indicator (●/○) + uptime via socket query; `--all` prints full detail block per agent (loop of single-agent view; `[name]` and `--all` are clap-mutex) |
| `bwoc list` | Registry view with runtime indicator + UPTIME column (5m12s when alive) + INBOX count; filters `--running` / `--status` / `--backend` / `--inbox-pending` (combinable); `--sort id\|inbox\|incarnated\|backend` (stable; default = registry order); `--count` (row count) / `--names-only` (bare ids for shell iteration); JSON gains `uptime_seconds` per agent (nullable); honored by both human + `--json` |
| `bwoc send <to> <msg>` + `bwoc inbox <agent>` | JSONL inbox at `<agent>/.bwoc/inbox.jsonl`. `send` body: inline `<msg>` OR `--file <path>` (clap mutex). `inbox`: `--watch` / `--clear` / `--limit` / `--json` / `--count` (envelope count for shell scripts); `--watch --json` streams one compact JSON envelope per line for log shippers; `--all` prints every agent's inbox concatenated with per-agent headers (refuses `--clear` / `--watch`). |
| `bwoc doctor` | Env + workspace diagnostic; `--auto` sweeps stale `agent.pid` / `agent.sock` / `inbox.cursor`; WARNs on oversize `agent.log` (10 MiB, `--auto` truncates) + oversize `inbox.jsonl` (5 MiB, WARN-only — user data); `--json` for stable CI-gating shape |
| `bwoc start <name>` (idempotent) | Flips registry + spawns `bwoc-agent --serve` if not running; `--no-daemon` opt-out; `--all` to mass-start every stopped agent; `--json` (requires `--yes`) emits `{ workspace, agent, daemon_spawned, daemon_pid, already_running, registry_updated }` for scripted lifecycle ops |
| `bwoc ping <name>` | CLI client for the daemon's PING command; `--all` mass-pings every agent (not-running labeled but not failed; protocol drift / connection errors → exit 1) |
| `bwoc chat <name>` (+ `--tmux`) | Auto-resolves backend from registry; exec's `bwoc spawn` |
| `bwoc dashboard` (TUI) | ratatui-based; agents pane + detail pane + 2s auto-refresh + `t/l/i` tmux hotkeys (chat / log -f / inbox --watch); `?` opens a centered hotkey help overlay; transient `last_action` feedback in footer; banner shows attention pending count when any agent has unread messages |
| Daemon-side inbox watch + cursor | Announces new envelopes to stderr; `.bwoc/inbox.cursor` survives restart |
| `--json` across read-only commands | `list`, `status`, `workspace info`, `workspace validate`, `check` |
| CI matrix | `ubuntu-latest` · `macos-latest` · `windows-latest` green on every push |
| Release pipeline (CalVer) | `release.yml` triggers on `v<YYYY>.<M>.<D>-<patch>` tag; 4 cross-platform binaries + `.sha256` to auto-created GitHub Release |
| Help system (in-binary) | 12 topics: `getting-started`, `backends`, `workspace`, `manifest`, `arc`, `lifecycle`, `daemon`, `messaging`, `persona`, `memory`, `doctor`, `script` |
| Shell completion | `bwoc completion <bash\|zsh\|fish\|powershell\|elvish>` via clap_complete |
| `bwoc init` writes `.gitignore` | Excludes daemon ephemerals (PID/socket/cursor) for user workspaces |
| `bwoc new --scope / --out-of-scope / --mindsets / --skills` | Persona substitution + mindset/skill stub seeding at incarnation |
| `bwoc new --json` | Emits `{ agent_id, target, registered_in, symlinks, mindset_stubs, skill_stubs, persona_filled }` instead of the human report. Useful for scripted multi-agent setup. |
| `bwoc init --json` | Emits `{ workspace, name, version, defaults, files_created }` instead of the human creation report. Pairs with `bwoc new --json` for end-to-end script chaining: `PATH=$(bwoc init /p --json \| jq -r .workspace) && bwoc new alpha --workspace "$PATH" --json …`. Last entry-point command to gain `--json` — JSON-everywhere matrix now covers every read AND write surface (interactive ones — spawn / chat / dashboard — excluded by design). |
| Shared `livecheck` module | Consolidated 5 copies of `signal_zero_alive` / `running_pid` / `query_uptime` / `format_uptime` / `inbox_count` |
| `bwoc-agent --serve` Windows stub | Compiles + runs default mode; `--serve` returns exit 2 with "Unix-only" message |
| `bwoc workspace info --path-only` | Emit just the resolved workspace root (one line, no decoration) — pairs with `cd "$(bwoc workspace info --path-only)"` shell idiom |
| `bwoc log <agent>` | Tails daemon stderr from `<agent>/.bwoc/agent.log`; `-f`/`--follow` for live streaming; `-n N` for last-N lines; `--clear` truncates in place |
| Per-workspace memory scaffold | `bwoc init` creates `.bwoc/memory/` with a README documenting the 4-tier scope hierarchy (per-agent / per-workspace / per-user / Tier 2) |
| `bwoc memory list \| show \| put \| search \| rm` | Full CRUD+search CLI for `.bwoc/memory/`: `list` (table + `--json` with `count` / `total_bytes` aggregates inline + `--count` / `--names-only` for script iteration + `--sort name\|size\|modified`), `show <name>` or `show --all` (`# === <name> ===` headers; `--json` array), `put <name>` (3 sources: inline positional > `--file` > stdin; modes: create / `--force` overwrite / `--append`; all writes atomic), `search <query>` (case-insensitive substring + `--json`), `rm <name>` (TTY confirm or `--yes`); all enforce flat-name + no-traversal, refuse README.md |
| `bwoc supervise <agent>` | Restart-on-crash supervisor for `bwoc-agent --serve`: spawn → wait → respawn on non-zero exit; rate-limit 10/min (`--max-restarts-per-min N`); clean exit (status 0) stops the supervisor. Stderr → same `agent.log` as `bwoc start`, so `bwoc log -f` works. SIGINT/SIGTERM via ctrlc exits cleanly. `--json` emits one structured event per action (watch_start / spawn / crash_respawn / clean_exit / rate_limit_hit / signal_stop / spawn_failed) to stdout. |
| `bwoc check --all` | Fleet-wide neutrality audit: iterates the workspace registry, runs `audit()` per agent, aggregates findings with per-agent sections + fleet summary; `--json` emits structured shape `{ agents[], summary }`. Exit 1 if any violations. |

### Remaining for ship

- **Cross-backend validation** — full uppāda + ṭhiti against all 5 backend CLIs in CI (proves Samānattatā); `bwoc-harness` (ollama) is the fifth.
- **Code signing** — Apple notarization + Windows Authenticode for release artifacts (user-cert authorization required).
- **Linux musl build** — `x86_64-unknown-linux-gnu` + `aarch64-unknown-linux-gnu` ship; musl (Alpine / distroless) can be added when demanded.
- **Memory mining tooling and pluggable Tier 2 backend interface.**
- **Windows named-pipe daemon path** — replace the cfg-gated stub with a real Windows implementation.

---

## Phase 3 — vaya + Interconnect

**Definition of done:** an agent's life ends cleanly; agents coordinate without a central authority.

### Shipped in Phase 3

| Item | Notes |
|---|---|
| `bwoc stop <name>` | 3-step escalation ladder: socket `STOP` → SIGTERM → SIGKILL (~3s wait between steps); idempotent; reports which step ended the daemon. `--all` to mass-stop every non-stopped agent (clap-enforced mutex with `name`). `--json` (requires `--yes`) emits `{ workspace, agent, daemon_outcome, registry_updated }` for scripted lifecycle ops. |
| `bwoc retire <name>` | Removes from registry; 3 file modes: default (delete dir), `--keep-files` (retain everything), `--keep-memory` (preserve just `memories/`, remove the rest — archives accumulated knowledge while letting the agent go). `--keep-files` and `--keep-memory` are clap-mutex. |
| `bwoc workspace prune` | Reconciles phantom registry entries vs orphan agent dirs; `--apply` removes safe drift; `--json` emits `{ phantoms, orphans, applied, removed }` for CI gating. |
| User → agent inbox (sammā-vācā Phase 0) | `bwoc send` + `bwoc inbox` ship as JSONL envelopes; foundation for agent → agent messaging. |
| Kalyāṇamitta-7 trust (5 of 5 steps) | All implementation steps shipped 2026-05-23: (1) `bwoc-core::Manifest` deserialization, (2) `bwoc check` evidence verification, (3) `bwoc trust <agent> read`, (4) daemon-level refusal at inbox poll behind `BWOC_TRUST_GATING=1` with sidecar `inbox.refusals.jsonl` and `bwoc inbox` merge, (5) CHANGELOG roll-up + TH parity. Spec: [`modules/agent-template/interconnect/trust.md`](../../modules/agent-template/interconnect/trust.md). |
| Agent → agent messaging (sammā-vācā Phase 1) | `bwoc send --from <agent>` writes sender identity into envelope; recipient daemon's trust gate evaluates against sender's manifest; refusals surface via `bwoc inbox` JSON merge. **Sāraṇīyadhamma 6** norms in [`interconnect/messaging.md`](../../modules/agent-template/interconnect/messaging.md) (+ `.th.md`). |
| Dual-mode `bwoc check` | Detects template (placeholder `manifest.name`) vs incarnation (real name). Template mode asserts placeholders exist + neutrality rules; incarnation mode asserts placeholders are gone (except runtime `{{taskId}}`) and skips neutrality checks. Closes the bug where un-personalized agents silently passed. |
| Interconnect routing — Track A | `.bwoc/interconnect/routes.toml` per-workspace, peer-declared (no central broker). `bwoc-core::routing` `Routes` type + resolve (exact `agent` → longest `namespace` prefix → `NotFound`); `send` consults it only on a local-registry miss, local-hit path byte-for-byte unchanged. Composes with the trust gate (a cross-workspace sender resolves as `unknown_sender` → refused), so it ships without Trust v2. Spec: [`interconnect/routing.md`](../../modules/agent-template/interconnect/routing.md). **Anattā / SN 22.59.** |
| Worktree lifecycle — Track B | `git_worktree` shell-out util (no `git2`/`gitoxide`). A `task-claimed` Saṅgha hook fires `git worktree add <worktreeBase>/<agentId>/<taskId> -b agent/<agentId>/feat/<taskId>` on claim; the `Task` struct is not extended — worktree location follows the `<worktreeBase>/<agentId>/<taskId>` path convention so cleanup is deterministic without parsing any agent log. |
| `bwoc retire` full vaya | Retire now ends an agent cleanly: worktree cleanup (worktrees under `<worktreeBase>/<agentId>/` removed via the git util), branch release (`agent/<agentId>/*` — `-d`, escalating to `-D` with the forced names surfaced), interconnect deregister (`Routes::remove_agent_routes` strips routes whose `agent` targets the retiree from `routes.toml`). Idempotent; respects the file-mode flags; `--json` extended additively. Completes the **vaya** DoD half. |

### Remaining for Phase 3 — deferred (off the DoD)

Both halves of the DoD are met: *coordinate without a central authority* (interconnect routing) and *an agent's life ends cleanly* (`bwoc retire` full vaya) — both shipped above. The sequencing rationale and the worktree-lifecycle / routing design decisions are in [`notes/2026-05-23_phase3-remaining-sequencing.md`](../../notes/2026-05-23_phase3-remaining-sequencing.md). What remains is deliberately off the DoD:

- **Trust v2** — signed envelopes / identity proof, warn-by-default refusal mode, cross-workspace messaging. Gated on v1 telemetry; the cross-workspace part builds on the routing layer.
- **Tier 2 memory** — two distinct pieces: the pluggable backend *interface* (also listed under Phase 2 remaining) and a *reference implementation*. Tier 1 file-based memory is complete.

---

## Shipped beyond Phase 3 — v2026.5.24-0 (2.2.0)

The following shipped after the Phase 3 DoD was declared met.

| Item | Notes |
|---|---|
| `bwoc-harness` — self-hosted agentic runtime | OpenAI-compatible model-API client + agentic loop; safety pipeline (guardrails → permission → sandbox); Unix-first v1 (compiles on Windows, untested there). Adds **ollama** as the fifth declared backend: `bwoc spawn --backend ollama` launches `bwoc-harness` against any Ollama / OpenAI-compatible endpoint. 8 production components. Spec: [`docs/en/HARNESS.en.md`](HARNESS.en.md). |

---

## Phase 4 — Reference Agents + Fleet

**Definition of done:** ecosystem viability proven; cross-vendor production fleet governance is achievable.

### Shipped in Phase 4

| Item | Notes |
|---|---|
| Fleet-governance spec | [`docs/en/FLEET-GOVERNANCE.en.md`](FLEET-GOVERNANCE.en.md) (+ `.th.md`) — Aparihāniya-dhamma 7 (DN 16) mapped to workspace-level operator practices: regular meetings, coordinated start/end, process-bound convention change, template-version honoring, vulnerable-agent protection, shared-resource respect, senior-agent protection. Observable signals named; automation deferred to v2 once telemetry justifies promoting signals to gates. |

### Goals (realized by external adoption)

These are realized by maintainers outside the original authors using the framework — the framework cannot reach them alone.

- Three or more reference agents in the wild, built by maintainers outside the original authors (per [`VISION.md`](../../VISION.md) one-year success).
- Fleet dashboard — Aparihāniya-dhamma 7 governance applied to a real multi-agent installation. **Spec landed 2026-05-23** ([`FLEET-GOVERNANCE.en.md`](FLEET-GOVERNANCE.en.md)); real-fleet validation pending.
- BWOC vocabulary (Yoniso manasikāra checks, Mattaññutā caps, Sīla baselines, Kalyāṇamitta trust scores) observed in codebases unaffiliated with this project (three-year success).
- Cross-vendor production fleet pattern in use at more than one organization.

---

## Cross-cutting (every phase)

- **Bilingual parity** — every spec doc has EN canonical + TH (and future languages); the bilingual-reminder hook gates this.
- **Backend neutrality** — every CLI feature works against any of the six declared backends; `/check-neutrality` gates this for `AGENTS.md`.
- **Doc-version + software-version stay consistent** — both auto-stamped on every Claude Code edit.
- **Open-source readiness** — every artifact a public contributor needs (CONTRIBUTING, SECURITY, CoC, LICENSE, VERSION, CHANGELOG, VISION, ROADMAP) is current and accurate.

---

## Non-Goals

See [`VISION.md` §Non-Goals](../../VISION.md#non-goals). Summary: BWOC is not a religion, not a runtime/SDK/LLM, not a replacement for DDD / Clean Architecture / SOLID, not vendor-aligned, and not a productivity framework.

---

## See Also

- [`VERSION.md`](../../VERSION.md) — current versions and SemVer policy.
- [`VISION.md`](../../VISION.md) — 1-year and 3-year success criteria.
- [`CHANGELOG.md`](../../CHANGELOG.md) — what shipped, when.
- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — how the components fit.
