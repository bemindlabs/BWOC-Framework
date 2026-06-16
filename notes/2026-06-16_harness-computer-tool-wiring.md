# 2026-06-16 — Computer-use P2: wire the tool into the loop, gated

Third computer-use slice. The P0 spike built the neutral action model + executor
seam; the P1 slice built the headless-browser executor. This slice **wires it
into the live loop** behind the `browser` feature and lands the **P2 security
gating** the prior notes flagged as required before any real use. Owner picked
the headless-browser direction over a full Xvfb desktop container.

## What changed

- **Registration (feature-gated)** — `tools/registry.rs::default_registry()` now
  registers `ComputerTool` when built with `--features browser`. Default builds
  pull zero browser deps and never register it (unchanged behavior).
- **Lazy launch** — `default_registry()` is sync (a `OnceLock` in
  `turn_executor` depends on it), but `BrowserExecutor::launch` is async. New
  `tools/browser.rs::LazyBrowserExecutor` wraps `Mutex<Option<BrowserExecutor>>`
  and launches Chromium on the first `execute()` — so opting into the feature
  costs nothing until an agent actually calls `computer` (and the gate allows it).
- **Anthropic native passthrough** — `provider/anthropic.rs::build_anthropic_body`
  emits the provider-native `computer_20250124` spec (via
  `computer::anthropic_tool_spec`, display geometry = `browser::DEFAULT_VIEWPORT`)
  for the `computer` tool instead of the custom-function `input_schema` shape.
  New `anthropic_beta_for(&tools)` attaches the `anthropic-beta:
  computer-use-2025-01-24` header on `complete`/`stream` only when a computer
  tool is present.
- **P2 security gating** (defense in depth, three layers):
  1. **Capability gate** — `computer` classifies as `Capability::Gated` (it was
     already, by the no-allow-by-omission default; now covered by the golden
     table + a `run_pipeline` test). Refused on any **Untrusted** turn.
  2. **Ask-by-default** — `policy/permission.rs::ASK_BY_DEFAULT_TOOLS = ["computer"]`.
     Resolves to `ask` even when `default_mode = "allow"`; only an explicit
     per-tool entry grants it (operator opt-in). A matching `allow` **pattern**
     can't grant it either (patterns may *tighten* to deny, never loosen) — else
     an incidental arg-substring match would bypass the gate. Caught in review.
  3. **Autoprocess refusal** — in the ask-by-default non-TTY path, computer-use
     **fail-safe denies** regardless of `default_mode`, so an autonomous run
     never silently drives the screen. Complements the t30 ambient-backend
     refusal at the agent layer.

## Decisions

- **Gate by tool name, register by feature.** The policy layer keys on the
  `"computer"` name, so the gating + tests are always compiled and CI-covered
  even though the live executor is feature-gated (no Chrome in the default
  matrix). The security contract is verified regardless of the feature flag.
- **Three-layer defense, not one.** Capability gate (untrusted turns) + ask
  (interactive) + non-TTY deny (autonomous) each close a different path. Any one
  alone leaves a gap (e.g. a trusted-turn allow-default autonomous run would
  otherwise drive computer-use unprompted).
- **Display geometry single-sourced** from `browser::DEFAULT_VIEWPORT` so the
  model's coordinate space matches the executor viewport.

## Status / deferred

- **Screenshot taint** — the screenshot result is already stamped
  `Principal::Tool { name: "computer" }` by construction (untrusted), so it taints
  the turn via the existing principal system; a dedicated image-block (`base64`)
  transport + explicit taint test is a later slice.
- **CI `browser` job** — still no Chrome in the matrix; the live smoke test stays
  `#[ignore]`. Verified locally with `cargo build --features browser` + the
  ignored smoke test. A dedicated Chrome job can run it later.
- Richer key-chord parsing (`ctrl+s` → modifiers) unchanged from P1.

## Related

- `notes/2026-06-15_harness-computer-use-spike.md` (P0), `notes/2026-06-15_harness-browser-executor.md` (P1)
- `crates/bwoc-harness/src/{tools/registry.rs,tools/browser.rs,tools/computer.rs,provider/anthropic.rs,policy/permission.rs,policy/mod.rs}`
