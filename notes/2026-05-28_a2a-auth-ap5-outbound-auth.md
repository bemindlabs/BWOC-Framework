# 2026-05-28 — A2A auth phase AP5: outbound client auth

Final slice of the A2A auth-phase epic (#80), after AP1 (#81, inbound auth),
AP2 (#82, safe bind), AP3 (#83, webhook delivery), AP4 (#84, rate/concurrency).
Lets a BWOC agent **authenticate to a remote** when it initiates A2A calls
(`bwoc a2a send` / `fetch-card`), presenting a per-peer bearer credential — and
only to a peer whose Agent Card declares it. Closes the epic.

## What changed

- **`creds.rs` (new)** — per-peer outbound credentials. Loads
  `<workspace>/.bwoc/a2a-credentials.json`, a flat map of remote **origin**
  (`scheme://host[:port]`) → bearer token. Lookups match by canonical origin
  (so `https://peer.example` covers `…/rpc` and `…:443/x`, not a different host
  or port). On Unix the file must be `0600` or stricter — a group/world-readable
  file is **refused** with a `chmod 600` message, matching the inbound
  `.bwoc/a2a.token` gate. Unparseable keys are skipped, not fatal.
- **`AgentCard::requires_bearer()`** — true iff the card's `security` list is
  non-empty and some `securitySchemes` entry is `{type:"http", scheme:"bearer"}`
  (scheme matched case-insensitively). Works for any A2A peer, not just BWOC.
- **`client::fetch_card` / `send_message`** — gained an `auth: Option<&str>`;
  present `Authorization: Bearer` when `Some`.
- **`bwoc a2a send` / `fetch-card`** — gained `--token` and `--workspace`. Token
  resolution: `--token` wins, else the per-origin creds-file entry.
  - `send` **honors the declared scheme**: it fetches the peer's card first and
    presents the token only if `requires_bearer()` — never leaking a credential
    to a peer that declared no auth. If discovery fails, it honors the
    operator's configured token rather than silently dropping it.
  - `fetch-card` presents a configured token best-effort (the card GET is public
    by spec, but a peer may protect its own card; there's no scheme to gate on
    yet).

## Decisions

- **Per-peer config file, not a flag/env-only.** Matches the realistic "talk to
  several peers" case; keyed by origin so one entry covers all paths/default
  port. `--token` stays as a per-call override. (Architect call over the simpler
  flag+env option.)
- **Fetch-card-then-gate for `send`.** Honors the issue's "declared scheme" and
  avoids leaking the token to a peer that didn't ask — at the cost of one extra
  public GET per send. (Architect call over present-if-configured.)
- **Discovery failure ⇒ honor operator intent.** If the card can't be read, a
  configured token *is* presented (the alternative — dropping it — guarantees a
  401 when the peer actually needs auth). Withholding happens only when the card
  was read and declared no auth.
- **`0600` gate on the creds file.** It holds peer secrets; same posture as the
  inbound token file (#81). No new dependency — `reqwest::Url::origin()` gives
  the canonical origin key.

## Verification

- `bwoc-a2a` 72 lib + 6 bin tests; full workspace + clippy green.
  - `creds`: origin match (path / default-port insensitive), miss, unparseable
    key skip, `0600` gate (lax refused).
  - `AgentCard::requires_bearer`: bearer yes, apiKey no, empty-`security` no,
    case-insensitive scheme.
  - `client`: authed `send_message` presents the bearer header (wiremock).
- Live against a real `bwoc a2a serve` (auth on ⇒ card declares Bearer):
  (A) `send` with a creds-file origin match → token presented → delivered;
  (B) `send` with no token → `401` (token never fabricated);
  (C) `--token` override → delivered;
  (D) `fetch-card` public GET → card;
  (E) a `0644` creds file → refused with a `chmod 600` message.

## Status

- **Epic #80 complete** — AP1–AP5 all landed. The A2A listener is safe to expose
  (auth, bind refusal, SSRF-guarded delivery, rate/concurrency caps) and the
  outbound client authenticates to peers.
- Possible later, not blocking: per-IP rate fairness, tunable limits via flags,
  delivery retry/backoff, richer auth schemes (OAuth2/mTLS) behind the same
  `securitySchemes` surface.

## Related

- Epic #80. Builds on AP1 (#81) inbound auth + the P4 outbound client (#76).
- `crates/bwoc-a2a/src/creds.rs`, `client.rs`, `types.rs`, `main.rs`;
  `crates/bwoc-cli/src/a2a.rs`, `main.rs`.
