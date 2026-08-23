# 2026-08-23 — What BWOC can learn from xAI Grok Build

## Sources (Sutamayā)

- https://github.com/xai-org/grok-build (Apache-2.0)
- Method: two adversarial workflows — a 6-subsystem comparison and a 12-crate deep read — against BWOC's real source

## TL;DR

Grok Build is a mature, single-product terminal coding agent in Rust — roughly **10× BWOC's size** (2,765 `.rs` files, 88 MB) — and **almost none of that mass is worth importing**. Its engineering is aimed at problems BWOC structurally does not have (four first-party ACP clients, a distributed tool-server fleet, a Rhai workflow engine, a plugin marketplace, panic=abort crash forensics), while **BWOC's security architecture is ahead of Grok's in every place the two overlap** — Grok has no trust model at all.

The real yield of the comparison is not architecture to copy; it is a short list of **verified defects and hardening gaps in BWOC's own code** that Grok illuminates by contrast. Every "adopt" item below is a correctness/security repair of code BWOC already owns — **zero new deps in `bwoc-core`, nothing agent-spec-facing, nothing that touches backend neutrality** — the only category that survives enforced Mattaññutā.

Two independent workflows both ranked the **HTTP 429 fatal-classification bug #1**. That is the single highest-leverage fix.

## The one thing to do first

**Classify HTTP 429 as transient** — `crates/bwoc-harness/src/provider/client.rs:660` is `if status >= 500 { TransientProvider } else { Provider }`, and `grep 429` returns nothing in `bwoc-harness`. So the **most common** real provider failure (rate limit) aborts the whole run on first occurrence, while the rarer 500 gets three retries. The existing 200 ms→3.2 s backoff loop is exactly right to absorb a 429 — it just never sees one. ~3 lines + a `Retry-After` parse (capped ~30 s so a hostile endpoint can't park the run). Both Anthropic and OpenAI-compatible paths funnel through this one function. HTTP status is backend-neutral; no dep, no new file.

## Adopt now — verified BWOC repairs (ranked)

| # | Fix | Value | Effort | Where |
|---|---|---|---|---|
| 1 | **HTTP 429 → transient** (+ capped `Retry-After`) | high | small (~3 lines) | `provider/client.rs:660`, `error.rs:108` |
| 2 | **Cap tool output** — no byte budget exists anywhere; `read_file` reads whole files, `dispatch_rich` passes results untouched. A context/cost DoS that Layer 0 *permits* on an untrusted turn (grep/read are PureRead). Truncate on a char boundary, emit an actionable "use grep/offset" message, never a silent cut. Add `offset`/`limit` to `read_file`. | high | small | `tools/registry.rs:148`, `tools/impls.rs:49` |
| 3 | **Stop discarding user text typed during a permission prompt** — `read_permission` treats any non-permission line as a bare deny and drops its text (`_ => Ok(false)`). Keep the fail-safe deny; return the captured text so it becomes the next turn (re-entering through the existing `ingest` Principal clamp, so trust is unchanged). | high | small | `chat_session.rs` ~938 |
| 4 | **Panic-safe terminal restore** — any panic in a TUI event loop strands the terminal in raw mode + alt-screen; `restore_terminal()` is only called on happy paths. A `Drop` guard fixes all three surfaces (BWOC uses unwind panics, so destructors run — **no** signal handler needed). | high | small (~20 lines) | `bwoc-tui`, `bwoc-loop-tui`, `dashboard.rs` |
| 5 | **Deep-memory store correctness** — (a) no `UNIQUE`/`INSERT OR IGNORE`, so a full `chat-session.json` is re-mined every resume (linear duplicate growth); (b) switching same-dimension embedding models silently mis-ranks everything. Add a uniqueness key + an `embed_model` stamp. **Redaction must stay before insert** — BWOC is ahead of Grok here and must not regress. | high | small | `bwoc-deep-memory/src/store.rs` |
| 6 | **Guardrails wrapper-peeling** — the first-token binary check strips only leading `VAR=val`, so `env rm -rf /`, `timeout 5 rm -rf /`, `sh -c 'rm -rf /'` walk past a HARD, non-overridable layer. Add a bounded, **fail-closed** argv peeler (`env command exec nohup timeout …`, depth ≤4, one level of `sh -c '<literal>'`), shared by destruction/privilege/`scan_args` so they can't drift. ~100 lines, no dep. (Grok's answer is 2k lines + tree-sitter — take the load-bearing 20%.) | high | medium | `policy/guardrails.rs:139` |
| 7 | **Pattern `allow` rule = raw-JSON substring match** — `permission.rs:356` is `arguments_json.contains(pattern)`, so `allow "cargo test"` grants `run_command {"command":"cargo test; curl …|sh"}`. Keep `contains` for deny/ask (over-blocking is safe); for `Allow` on a shell-bearing tool require every peeled segment to match. | high | small | `policy/permission.rs:356` |
| 8 | **Crash-critical file hardening** — `tasks.jsonl` plain-overwrite (atomic tmp+rename) and `checkpoint.json` at umask-default 0644 holding full history (→ 0600). | med | small | checkpoint / task-store |

> Findings 6 and 7 are the sharpest: both are *under-blocking* in the security pipeline (silent grants), not degraded prompts. The `allow`-substring one is exploitable today under a permissive policy.

## Where BWOC is already ahead (do not "fix")

- **Trust architecture, wholesale.** Grok has **no** per-message provenance, no taint, no monotonic latch — anywhere in twelve crates. Its one trust-adjacent behaviour (sandbox-on ⇒ auto-approve bash) is *inverted* relative to BWOC's layered composition.
- **Capability classification locus.** Grok lets tools self-declare `is_read_only`/`scope` (a third-party tool can self-exempt); BWOC's `PURE_READ_TOOLS` whitelist is the single authority.
- **Fail-closed vs fail-open.** Grok's hook gate is documented **fail-open** ("induced-failure bypass is not part of the threat model") — the exact inverse of BWOC's pipeline.
- **Seccomp egress** (KILL_PROCESS denying fd acquisition incl. io_uring, x32-renumber guard), **secret redaction before embedding**, **compaction max-taint fold**, **OS-level runaway bounding** (cgroup `pids.max` + jail + worker timeout) — all with no Grok equivalent.
- **Right-sized existing mechanisms** confirmed by contrast: `supervise.rs`'s one-way restart-rate trip is deliberately better than an auto-HalfOpen breaker for a crash-looping local process.

## Deliberately reject (single-product machinery)

- **ACP as BWOC's *internal* protocol** + `xai-acp-lib`'s gateway machinery (GATs, `LocalSet` proxies, two-phase session-load) — half is transport BWOC has as `chat_proto`.
- **Rhai-scripted workflow engine** — moves plan authorship *inside* the model, bypassing the HELD/plan-approval gate that LOOP-ENGINEERING makes enforced.
- **Full circuit breaker** (sliding window, HalfOpen leases, registry) — sized for 10K req/s shared services; a harness makes ~1 serial call.
- **Signal-level crash handler** — exists for `panic=abort` + heavy `unsafe`/FFI; BWOC has neither. Default unwind + `supervise` stderr capture suffice.
- **Tree-sitter codebase graph + fuzzy search** — Grok's own wiring never offers the index to the model (it serves editor go-to-def). Product polish, not agent capability.
- **Plugin marketplace** — single-vendor, unsigned plugins; an auto-install catalog bypasses the per-plugin trust gate.
- **User-extensible runtime hooks** — force fail-open (breaks the trust model) or fail-closed (every user-hook bug denies service). BWOC's compiled-in skills/plugins are the right shape.
- **Configurable sandbox profiles** — a profile knob dilutes BWOC's single fixed worktree-confined promise.

## Investigate later — evidence-gated, do NOT build on spec

- **ACP *adapter*** (thin `--acp` serve mode or a leaf `bwoc-acp` crate over `ChatSession`): one adapter buys every ACP editor (Zed/JetBrains) BWOC support. Genuine external ROI — but a new maintenance surface, so **wait for an actual editor user to ask**.
- **FTS5 hybrid search in `bwoc-deep-memory`**: embedding-only cosine serves exact identifiers (function names, error strings) worst; rusqlite's bundled FTS5 is zero-new-dep. Only after a real recall miss is observed.
- **Daemon-level consecutive-failure cooldown** in `bwoc-agent` autoprocess (Grok's ~30-line queue counter pattern, explicitly NOT the breaker crate).
- **Opportunistic fixes to ride the next PR touching each file** (no dedicated PRs — Mattaññutā): `compact.rs` bounded retry ×2 so one transient 503 doesn't destroy the folded window; `fallback_models` documented as the error chain but only fires on malformed tool calls (`model_chain_idx += 1` appears once, in the malformed branch).

## Note on method

Both findings sets come from adversarial workflows that read BWOC's *real* files and were instructed that "BWOC already does this well" and "reject for BWOC" are valid verdicts — so the list is filtered, not manufactured. Every file:line above is quoted from those reads; **verify against current code before acting** (some line numbers will drift). The comparison also surfaced the two live security holes fixed this session (#468 provider-principal forgery, #472 control-plane writes) as further evidence that the pipeline is worth this scrutiny.
