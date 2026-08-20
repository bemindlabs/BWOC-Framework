# 2026-08-20 — Principal forgery clamp (#452 Slice 2) + a live trust escalation

Slice 2 of the L3 middle trust tier: make `Principal::A2aSender { verified: true }`
— the identity the act-as-user tier keys on — **unforgeable**. Reviewing it
surfaced something bigger: a **live trust escalation** in which a provider
response could stamp itself `SelfAgent` and flip a turn to Trusted. That is not
gated behind the inert L3 work; it applies to every harness run today.

## The live vulnerability (found by the review, verified by probe)

`Choice { pub message: ChatMessage }` derives `Deserialize`, and `ChatMessage`
carries `principal`. The non-streaming provider path does
`resp.json::<ChatCompletion>()` → `Ok((choice.message, usage))` →
`history.push(completion)`, so **the response body chose its own provenance**.

Probed directly against the real types before fixing:

```
injected {"kind":"self_agent"}                        -> principal=SelfAgent   trust=Trusted
injected {"kind":"a2a_sender",...,"verified":true}    -> principal=A2aSender{verified:true}
```

`SelfAgent` is one of only **two Trusted principals**. `scan_turn_trust` excludes
only `Assistant` as derived, so a forged `SelfAgent` counts as clean ingress →
the turn scans **Trusted** → the Layer-0 capability gate becomes a no-op →
`run_command`, `git push`, network egress all unlock on a turn that should have
been untrusted.

Reachable by any endpoint that is hostile, compromised, or merely MITM'd — and
an `ollama` endpoint is **plain http by default**. It is the default code path
(`stream: false`); the streaming path was never affected because it rebuilds via
`ChatMessage::assistant`.

**Fix:** a completion *is* the agent's own model turn, so stamp that and ignore
the wire — `choice.message.with_assistant_provenance()` in `call_provider_once`.
This also makes the two provider paths agree.

## What Slice 2 itself changed

- **`ChatMessage::ingest`** now clamps `A2aSender` (as well as `SelfAgent`) to
  `Unknown`: a message may not declare *itself* signature-verified.
- **`ChatMessage::verified_a2a_sender`** is the one constructor that may stamp
  `A2aSender { verified: true }` — reserved for code that has just verified a
  signature. This split *is* the control.
- **`RunState::load_from`** and **`chat_session::load_session`** clamp reloaded
  identities. The latter matters *more*: `chat-session.json` lives at
  `<workdir>/.bwoc/`, **inside the agent's own writable worktree**, reachable by
  its `write_file` tool under `Capability::WorktreeWrite` — whereas the run
  checkpoint normally sits outside. One confined worktree write could otherwise
  plant a verified identity for a later turn to read back.

All clamps are taint-preserving (`Unknown` is Untrusted too), so no trust verdict
changes for ordinary runs; what dies is the forged *authority*.

## The collision this slice had to resolve

Adding the clamp immediately broke Slice 1's tests — which was the useful signal.
`scan_authenticated_actor` requires a matching `A2aSender` in history, so a
blanket clamp with no mint path would have left act-as-user **permanently and
silently unreachable**. Hence `verified_a2a_sender`: the wire path clamps, the
post-verification path stamps. The design workflow had flagged exactly this as
"the collision"; it showed up in practice within minutes.

## Review outcome — 9 raised, 6 confirmed, 3 distinct issues

1. **(high/critical) Provider response forgery** — above. Verified by probe
   before fixing, so the test asserts a real precondition, not a hypothetical.
2. **(med) `chat_session::load_session` unclamped** — the twin reload entry
   point, on a *more* attacker-reachable file than the one already clamped.
3. **(med) I unpinned a guard while fixing tests.** Rerouting `a2a(_, false)`
   through `ingest` made the unverified case arrive as `Unknown`, so nothing
   exercised the `*verified &&` guard in `scan_authenticated_actor` —
   mutation-confirmed: deleting that guard kept all 506 tests green. The helper
   now builds a genuine `A2aSender { verified: false }` via serde round-trip.

## Every fix proved load-bearing by mutation

Each control was removed and the suite re-run; each produced a red test naming
the defect, then green on restore:

| Removed | Result |
|---|---|
| `ingest`'s A2aSender clamp | red — "must not preserve a wire-claimed A2aSender" |
| checkpoint reload clamp | red — "must not survive reload as a verified identity" |
| `.with_assistant_provenance()` **call site** | red — "must not survive into history" |
| `*verified &&` guard | red — the unverified-sender test |

The call-site test matters especially: a unit test of the helper alone would
have stayed green while the one line that calls it was deleted — the exact
"nothing asserted it" failure mode that let `bwoc-harness` go missing from every
release archive (#460).

## Status

508 harness tests pass; `clippy --all-targets --features test-redteam -D warnings`
clean. ActAsUser remains inert in production (the actor is always `None` until
Slice 4), so this slice still changes no production behaviour — **except** the
provider-response fix, which is live and closes a real escalation.

## Related

- Slice 1: `notes/2026-08-19_actas-capability-slice1.md` (#457). ADR: issue #452.
- Remaining: Slice 3 (`evaluate` carries the verified `from`), Slice 4 (daemon
  minting + out-of-band session actor + peer pin).
