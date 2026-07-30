# 2026-07-30 — Fleet TUI: per-agent manifest backend/model

Each pane in the multi-agent fleet TUI now drives its agent with that agent's
**own** backend and manifest-resolved model/endpoint, instead of one shared
operator-supplied config for the whole fleet.

## What changed

- `session::AgentInfo` gains `path` (already emitted by `bwoc list --json`;
  `#[serde(default)]` tolerates an older `bwoc`). It locates each agent's
  `config.manifest.json`.
- `SessionConfig::for_agent(&AgentInfo)` reads `<workdir>/<path>/config.manifest.json`
  and layers `primaryModel` → `model`, `baseUrl` → `endpoint`, and the agent's
  registry `backend` over the fleet defaults. A pure `with_overrides` helper does
  the merge (unit-tested); missing/empty fields keep the default, `"auto"` passes
  through for harness-side resolution.
- `session::is_harness_drivable(backend)` mirrors
  `bwoc_cli::spawn::Backend::uses_harness` (`ollama | openai-compatible |
  openrouter | litellm`). `Fleet::open` gates on it: a vendor-CLI agent gets a
  pane with a one-time hint ("open with `bwoc chat <id>` directly") but **no**
  harness session — previously the fleet would have mis-driven it with the shared
  harness backend.
- `chat.rs` fleet comment updated: the named agent now seeds only the fleet
  *defaults*; per-agent manifest override is no longer "a later slice".

## Decisions

- **Resolution lives in `bwoc-tui`, not `bwoc-cli`.** The TUI already depends on
  `bwoc-core` (hence `manifest::Manifest`), and `bwoc list --json` already carries
  `path` — so no CLI change and no new `bwoc-cli` dependency edge. Dep-quarantine
  intact.
- **Duplicate the harness-backend list** (4 strings) rather than depend on
  `bwoc-cli` for `uses_harness`. The TUI must not pull in the CLI crate; the
  mirror carries a comment pointing at the source of truth.
- **Defaults, not hard failure, on a missing manifest.** A pane without a readable
  manifest still opens on the fleet-default model/endpoint — the harness has its
  own fallback anyway. Mattaññutā: don't fail a whole fleet over one agent's gap.

## Status / deferred

- Sidebar still shows id + status glyph only; a per-agent backend/model tag is
  deferred (not needed for correctness).
- `@mention` routing, inline tool-actions, and header cost/%context remain later
  phases.

## Related (links)

- [[2026-07-29_tui-fleet-multi-agent]] — the fleet layer this extends.
- [[2026-07-30_tui-scrollback]] — sibling P2 slice.
