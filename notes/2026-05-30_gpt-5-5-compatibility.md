# 2026-05-30 — GPT-5.5 compatibility pass

Updated BWOC's model-facing surfaces for OpenAI GPT-5.5 without changing the
harness API contract.

## What changed

- Refreshed the `bwoc new` model picker for Codex and OpenAI-compatible
  backends so GPT-5.5 appears first.
- Wired optional `reasoningEffort` through the manifest and OpenAI-compatible
  provider request body as `reasoning_effort`.
- Extended the backend-neutrality hardcoded-model detector to catch GPT-5
  family model IDs in instruction files.
- Documented GPT-5.5 usage for OpenAI-compatible harness runs, including
  `primaryModel: "auto"` / `autoModels` ordering and the current
  `/v1/chat/completions` compatibility boundary.
- Updated the template manifest description for `autoModels` to steer
  OpenAI-compatible pools toward high-capability-first ordering.

## Decisions

- Did not silently rewrite `AGENTS.md` around GPT-5.5. The agent instruction
  file must stay backend-neutral and model-ID-free.
- Did not migrate `bwoc-harness` to the Responses API in this pass. OpenAI's
  guidance recommends Responses for reasoning models, but BWOC's current
  provider contract is intentionally OpenAI-compatible so Ollama and local
  endpoints keep working. A native Responses adapter should be its own design
  change.

## Related

- OpenAI: `https://openai.com/index/introducing-gpt-5-5/`
- OpenAI API guide: `https://developers.openai.com/api/docs/guides/latest-model`
- OpenAI prompting guide: `https://developers.openai.com/api/docs/guides/prompt-guidance`
