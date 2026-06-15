# 2026-06-15 — Computer-use harness P0 spike

First slice toward a computer-use harness (Phase 7 candidate): give the
harnessed agent a `computer` tool (screenshot + mouse/keyboard) alongside the
existing bash/edit tools. This spike answers the **architecture** question only —
does a backend-neutral action model hold, and does it map to the provider wire
format? — before committing to the GUI/container/security investment.

## What changed

New `crates/bwoc-harness/src/tools/computer.rs` (module registered in
`tools/mod.rs`, **not** added to `default_registry` — zero behavior change):

- `ComputerAction` — backend-neutral action enum (screenshot, mouse_move, click,
  double_click, type, key, scroll, wait). Tagged with `action` + sibling fields,
  i.e. exactly the JSON the model emits as tool input, so `from_value` parses a
  tool call directly. Vocabulary = intersection of the Anthropic `computer` tool
  and the OpenAI CUA action set.
- `ComputerExecutor` trait + `MockExecutor` — the executor seam. Mock returns a
  1×1 PNG for `Screenshot` and acks everything else, so the loop + eval run with
  **no GUI / no display / no container**.
- `anthropic_tool_spec()` + `ANTHROPIC_COMPUTER_BETA` — the Anthropic
  provider-native serialization: `{"type":"computer_20250124","name":"computer",
  display_*_px}` + the `anthropic-beta: computer-use-2025-01-24` header. This is
  the real architectural fork — the current `Tool` model only knows custom
  function tools (`{name, description, input_schema}`); a provider-defined tool
  keyed by `type` is a new shape.
- `ComputerTool<E>: ToolImpl` — custom-function fallback path for non-Anthropic
  backends; dispatches the same parsed action into the executor.

9 unit tests (action serde round-trip, Anthropic native-spec shape, mock
executor, tool dispatch + bad-action rejection). fmt + clippy clean.

## Decisions

- **Spike proves mapping, not the live loop.** It does NOT touch `turn_executor`
  or `default_registry`. Wiring computer-use into the agent loop and the
  provider's `tools` array (native-tool passthrough + beta header) is the next
  slice. Keeping the spike isolated makes it reversible (Mattaññutā) and lets the
  design be judged before infra cost (Yoniso manasikāra).
- **Neutral model, not Anthropic-shaped.** The OpenAI CUA loop differs
  (`computer_call` → `computer_call_output` with a screenshot); a neutral enum +
  per-provider adapter keeps Samānattatā (treat backends equally).
- **No image/base64 deps.** Mock PNG is a hardcoded constant; `Observation`
  carries raw bytes. Base64 `image`-block transport is deferred to wiring time.

## Status / deferred (the rest of the plan)

- **P1** — real executor in a Linux container (Xvfb + WM + action server over a
  unix socket); trusted mode only.
- **P2** — security: taint screenshots (untrusted input → prompt-injection
  surface), gate `computer` behind a high capability tier, `ask` every action by
  default, **autoprocess refuses computer-use** unless explicitly granted (mirror
  t30's cli-ambient refusal). This is where Phase 5's seccomp egress containment +
  capability gate pay off.
- **P3** — OpenAI CUA provider parity + eval scoring of computer-use fixtures
  (ties into the eval framework, option B).
- **Open ROI question for the owner:** full GUI automation vs a lighter
  headless-browser tool that covers most use-cases at far lower maintenance/
  security weight.

## Related

- `crates/bwoc-harness/src/tools/computer.rs`
- `crates/bwoc-harness/src/provider/anthropic.rs` (tool serialization fork point)
