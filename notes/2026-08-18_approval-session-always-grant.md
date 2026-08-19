# 2026-08-18 — Approval "Always allow" → session-scoped grant (#409)

The non-TTY approval flow (`policy/approval.rs`, #392) accepted an
`ApprovalDecision.always` field and the bwoc-mcc console shipped an **Always**
button, but the harness only *logged* `always: true` and treated it as a
one-shot — the next identical call re-prompted. The UI over-promised
persistence the backend didn't deliver. This makes "Always" actually skip the
prompt for the rest of the session.

## What changed

- `policy/permission.rs`: `Policy` gains `session_grants:
  Arc<Mutex<HashSet<(String, u64)>>>` — session-scoped "always allow" grants
  keyed on `(tool, hash(args))`. `apply_mode`'s `Mode::Ask` arm now:
  - **checks** the grant set *before* prompting (TTY or console) and returns
    `Allow` on a hit — this is what makes "Always" durable within a session;
  - **records** a grant when the operator answers `allow && always`, replacing
    the old log-only branch.
- `policy/approval.rs`: updated the `always` doc to describe the session grant
  (was "persisting it is a later slice … one-shot").
- 4 unit tests: repeat-same-args skips the channel; different-args re-prompts
  (args-specific); grants don't leak across `Policy` instances (separate
  sessions); a cloned `Policy` shares the grant (same session / spawned worker).

## Decisions

- **Session-scoped, in-memory — NOT written to `harness-policy.toml`.** A
  confined harness rewriting its own policy file is a self-escalation smell; a
  durable allow rule must stay human-authored. Durable persistence, if wanted,
  belongs in the console (bwoc-mcc), tracked there. (Chosen with the architect
  over the tool-level and durable-file alternatives.)
- **Keyed on `(tool, exact args)`, not tool-only.** Clicking "Always" on
  `write_file {path:a}` grants exactly that call, not all `write_file` — tightest
  blast radius. Args are hashed (`DefaultHasher`) so the set bounds memory and
  never retains potentially-sensitive full argument strings.
- **Fail-safe preserved.** The grant only ever turns a would-be *ask* into an
  operator-approved *allow*; a `deny` resolved upstream never reaches the
  `Mode::Ask` arm, so it can't be weakened. A poisoned lock reads as "not
  granted" (re-prompt) and drops on record — never fails open.
- `Arc`-shared so a cloned `Policy` (spawned worker in the same session) honours
  a grant taken on the original.

## Status / deferred

- Durable cross-restart persistence is intentionally out of scope (belongs in
  the console). Issue #409's option (2) — hiding the console button until
  persistence lands — is now moot: the button does something lasting (for the
  session).

## Related

- Approval channel: #392. Console button: bemindlabs/bwoc-mcc v0.1.2.
