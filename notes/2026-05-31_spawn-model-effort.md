# 2026-05-31 — `bwoc spawn` forwards the model + reasoning-effort

Three follow-ups from the auto-model / reasoning-effort work, all in `bwoc spawn`:

1. **Harness backends now receive `--model`.** `bwoc spawn --backend ollama|openai-compatible` previously execed `bwoc-harness` without `--model`, so it fell back to the harness default (`gemma4`) and ignored the agent's `primaryModel` entirely — including the `"auto"` sentinel. Flagged in #120 review. Now the manifest's `primaryModel` is forwarded, so `spawn` reaches auto-resolution exactly like `bwoc run`.
2. **Claude spawn honours `reasoningEffort`** → `claude --effort <level>`.
3. **Codex spawn honours `reasoningEffort`** → `codex -c model_reasoning_effort=<level>`.

## What changed (all in `crates/bwoc-cli/src/spawn.rs`)

- Manifest is now loaded once at the top of `spawn()` and reused for `--endpoint`, `--model`, and `reasoningEffort`.
- Harness branch appends `--model <primaryModel>` unless `--extra` already carries `--model`/`-m` (`extra_has_model`), avoiding a duplicate-flag clap error.
- Vendor branch appends `Backend::vendor_effort_args(effort)` unless `--extra` already sets it (`extra_has_effort`). Backends with no effort control emit a one-line `note:` and pass nothing.
- New `Backend::vendor_effort_args` + `extra_has_model` / `extra_has_effort` helpers, with unit tests.

## Decisions

- **Effort flags verified against the installed CLIs, not guessed** (Yoniso): `claude --help` → `--effort low|medium|high|xhigh|max`; `codex --help` → `-c key=value` overrides (`model_reasoning_effort` is the standard config key); `kimi --help` → only a boolean `--thinking`/`--no-thinking` (no level); `agy --help` → no effort flag.
- **Kimi and Antigravity are NOT wired.** Kimi has only on/off thinking and Antigravity has nothing — forcing a free-form `reasoningEffort` onto a boolean (or a non-existent flag) would be a lossy/fabricated mapping. They emit a `note:` and ignore the field. *(Yoniso + Mattaññutā — don't invent a mapping the CLI doesn't have.)*
- **Value forwarded verbatim.** `reasoningEffort`'s value space is backend-specific by design (Claude `max` vs Codex `high`); the operator sets a level their backend accepts.
- **Harness backends carry effort via the manifest, not the CLI.** `bwoc-harness` already reads `reasoningEffort` and sends `reasoning_effort` on the request, so `spawn` only needs to forward `--model` for them.
- **`--extra` always wins.** An explicit `--model` / `--effort` in `--extra` suppresses the manifest-derived one.

## Status / deferred

- Kimi binary-`--thinking` and Antigravity have no effort-level passthrough (no suitable CLI surface). Revisit if those CLIs add a level flag.

## Related (links)

- `crates/bwoc-cli/src/spawn.rs` (`vendor_effort_args`, dispatch)
- `crates/bwoc-cli/src/run.rs` (the `bwoc run` analogue — already forwards `--model` and Claude `--effort`)
- `crates/bwoc-harness/src/main.rs` (harness reads `reasoningEffort` itself)
