# 2026-07-26 — Harness multimodal input: provider-neutral image serialization

First slice of the **multimodal-input** gap (LLM-backend audit item 8 — a local audit-list number, *not* GitHub Issue #8, which is unrelated `bwoc update`): a `ChatMessage` can now carry base64 images, serialized correctly on **both** provider paths. The end-to-end screenshot wiring is deliberately deferred (see below).

## Why split here

The only in-harness image *producer* is the browser/computer tool's `Observation::Image(Screenshot)`, and it's feature-gated **off by default**. More importantly, routing those bytes into a `ChatMessage` crosses the **Phase-5 re-exec turn-executor IPC boundary** (`execute_proceeded` marshals tool output as a `String` across an isolated child process) — a security-critical protocol change that can't be verified on this Mac. So the honest, Mattaññutā-correct slice is the provider-neutral *serialization capability* now (self-contained, unit-testable on Mac), with the IPC wiring as a bemind-verified follow-up — the same way the MCP HTTP transport was split from the MCP protocol bump.

## What changed

- **`ChatMessage.images: Option<Vec<ImageBlock>>`** (`provider/types.rs`) — additive, `#[serde(default, skip_serializing_if)]` → retained on disk, inert for existing flows. `ImageBlock { media_type, data }` where `data` is base64 (no `data:` prefix). Builder `with_images`; `images: None` threaded into all 7 constructors.
- **Anthropic path** (`provider/anthropic.rs`) — `anthropic_image_blocks()` renders `{type:image, source:{type:base64, media_type, data}}`. User turns append image blocks after the text block; tool results switch from plain-string content to the block-array form (`[text?, image…]`) **only when images are present** — text-only results stay byte-identical.
- **OpenAI-compat path** (`provider/client.rs`) — `EgressContent` enum with a custom `Serialize`: `Text(&str)` serializes as a string (unchanged), `Parts{text, images}` as `[{type:text,…}, {type:image_url, image_url:{url:"data:<mt>;base64,<data>"}}]`. `EgressMessage::from` picks Parts iff the message has images. Every OpenAI-compat provider (Ollama/openrouter/litellm/gemini) rides this.
- **`tools/computer.rs`** — corrected three now-stale "later slice" comments to reflect that serialization landed and only the IPC wiring remains.
- **Docs (EN+TH parity)** — HARNESS provider section documents the provider-neutral image input + the deferred wiring.

## Decisions

- **Additive field, not a content-block enum.** Turning `content: Option<String>` into a rich block enum would touch every constructor, the trust plumbing, and the whole codebase. A parallel `images` field (like `thinking_blocks`) is the minimal change and keeps text-only serialization byte-for-byte identical.
- **base64 stored as a `String`, no new dep.** `ImageBlock.data` is already base64; encoding raw bytes happens at the (deferred) executor-wiring stage, so this PR adds no `base64` crate and doesn't touch dep-quarantine.
- **Images allowed on any role at serialization time.** Whether a screenshot returns as an Anthropic `tool_result` block or an OpenAI *user* message is a per-provider semantic the wiring PR will settle; the serializer just renders whatever is set.

## Verification

macOS: fmt + clippy (`--workspace` and `-p bwoc-harness --features test-redteam`) clean with `-D warnings`; workspace tests pass, 0 failed. New unit tests: Anthropic user-image block, Anthropic tool_result array-vs-string, OpenAI image_url parts-vs-string, on-disk `images` round-trip. No live vision call (no consumer wired yet; nothing to run).

## Status / deferred

- **Deferred (bemind-verified follow-up):** wire `Observation::Image` → base64 `ImageBlock` through the re-exec turn-executor IPC output protocol into the `tool_result` message. Security-critical (widens the isolation boundary's marshalling); needs Linux verification.
- Also still open: MCP Streamable HTTP transport. Structured output (audit item 5) stays parked. With this slice, the "go next all" LLM-backend roadmap's serialization surfaces (effort, max_tokens, usage, caching, thinking incl. streaming, MCP negotiation, multimodal) are all landed.

## Related

- `provider/{types,anthropic,client}.rs` · `tools/computer.rs` · prior harness sprints #380–#384 · claude-api skill (image content blocks).
