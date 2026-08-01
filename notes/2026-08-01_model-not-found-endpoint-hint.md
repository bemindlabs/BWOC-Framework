# 2026-08-01 — `ModelNotFound` names the endpoint cause too (#402)

A wrong OpenAI-compatible endpoint (e.g. `baseUrl` missing the trailing `/v1`)
returns HTTP 404 on `/chat/completions`, which the client maps to
`HarnessError::ModelNotFound`. Its message said only "check the model tag" —
misdirecting the operator when the real fault is the endpoint.

## What changed

- Broadened the `ModelNotFound` Display message (`error.rs`) to name both
  causes: verify the model tag (e.g. `ollama list`) **and** that the endpoint
  URL is correct (OpenAI-compatible paths end in `/v1`).
- Added a unit test asserting the message mentions the model, "model tag", and
  `/v1`.

## Decisions

- **Message broadening, not an endpoint probe.** A bare 404 can't distinguish
  wrong-tag from wrong-endpoint; #402 offered either a one-line honest message or
  a `GET /models` probe to disambiguate. Chose the message (Mattaññutā — no extra
  request/latency, covers every 404 site: completion, stream, validate). The
  precise probe is deferred and noted in #402.
- **Backend-neutral wording.** `ollama list` stays as an *example*; the endpoint
  hint (`/v1`) applies to any OpenAI-compatible provider (ollama / openai /
  openrouter / litellm).

## Related (links)

- Issue #402. Found during the fleet-TUI headless verification session.
- `crates/bwoc-harness/src/provider/client.rs` — the 404→`ModelNotFound` sites.
