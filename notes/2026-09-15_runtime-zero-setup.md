# 2026-09-15 — Runtime R1: zero-setup `bwoc`

Phase R1 of "BWOC as its own coding agent". Bare `bwoc` on a terminal now opens a harness chat session in the current directory, with no workspace, registered agent or manifest. The provider comes from flags, env, a project or user `config.toml`, or auto-detection. `bwoc auth` stores API keys. The harness builds a real system prompt for project sessions and gives agent sessions a condensed persona. Built on `fix/chat-harness-entry` (R0, PR #519).

## What changed

- **`crates/bwoc-cli/src/runtime.rs`** (new): `[runtime]` config parse (`schema_version` through `bwoc-core::schema`, a future revision is refused), layer gathering, field-wise merge, provider resolution with injectable probes, the no-provider help, `bare_route`, and `run_session`, which hands a `ProjectSession` to `bwoc-tui`.
- **`crates/bwoc-cli/src/auth.rs`** (new): `bwoc auth set <provider>` / `bwoc auth status`. Edits use `toml_edit` (already in the lockfile through the harness's `toml` 0.8) and a temp-file + rename write with mode `0600`.
- **`main.rs`**: top-level `--backend` / `--model` / `--endpoint` (rejected with a subcommand, and they need a TTY), the `auth` and `about` subcommands, and the TTY-gated bare route.
- **`bwoc-tui`**: `TuiArgs.project: Option<ProjectSession>`. The same `run()` and `harness_argv`, plus `project_argv` (`--agent`, `--max-tokens`, `--session-file`). There is no fork of the TUI.
- **`bwoc-harness/src/system_prompt.rs`** (new) plus `prompts/coding_agent.md`, wired into `run_chat_mode` (chat and headless). Batch `run()` keeps `load_system_prompt`. New `--max-tokens` flag. `openai-compatible` sends `[openai-compatible] api_key` when one is stored.
- **`doctor.rs`**: the `/api/tags` probe is now `pub(crate)` and sends HTTP/1.0.

## Decisions

- **TTY gate is stdin AND stdout.** A pipe on either side prints the banner as before. The integration test compares bare output with `bwoc about` (same bytes, no ANSI escapes, no session attempt). A manual `cmp` against a pre-change capture was byte-identical. `bwoc about` exists because a terminal would otherwise never see the banner again.
- **Flags on bare `bwoc`, not only env.** clap takes them cleanly on `Cli`. Using them with a subcommand or without a TTY exits `2`, so no existing invocation changes meaning: none of these flags existed at top level before.
- **Env names `BWOC_BACKEND` / `BWOC_MODEL` / `BWOC_ENDPOINT`.** None existed before.
- **Merge guard.** A layer that names a different backend from the winner contributes no model, endpoint or max_tokens. Otherwise `BWOC_BACKEND=ollama` over a user config written for `anthropic` would send a vendor model id to Ollama.
- **One model constant.** `DEFAULT_ANTHROPIC_MODEL` in `runtime.rs`, used only when auto-detect picks `anthropic` and nothing names a model. Neutrality is an `AGENTS.md` rule, and this is Rust runtime code.
- **The project config search stops at the git root.** Outside git, only the cwd counts, so a stray `.bwoc/config.toml` higher up never configures an unrelated directory.
- **`openai-compatible` has no env var for its key.** A generic `OPENAI_API_KEY` would be sent to whatever endpoint the session points at.
- **The session file lives in `~/.bwoc/sessions/<sha256(cwd)[..8]>.json`.** The harness default `<workdir>/.bwoc/chat-session.json` would drop a `.bwoc/` into every repository.
- **Session kind = `config.manifest.json` in the workdir.** The harness already keys manifest-driven settings off this file. Project sessions skip Tier-1 `MEMORY.md` recall, because a repo's own `memories/` is not agent memory.
- **Public boundary is enforced twice.** A bwoc-connect public workdir carries a copied manifest, so it is an agent session: workdir `AGENTS.md` only, and no persona, since the slots are not copied. Independently, `instruction_dirs` returns only the workdir for any path containing `.bwoc/public`, and the environment block (which holds the absolute path) is left out there.
- **Caps.** 32 KB for project instructions, cut farthest first with a byte-count note. 8 KB for persona/mindsets. 400 bytes per mindset summary. Each `git` call gets 2 s, with its stdout drained on a thread so a large `status` can't stall.

## Alternatives considered

- **Resolving the runtime inside the harness**, where env is already read. Rejected: the TUI needs the backend and model before spawning, and auto-detection belongs in the zero-dep CLI side.
- **Hunk-level commits that each compile with every module wired.** File-level commits were chosen instead. `auth.rs` / `runtime.rs` are not compiled until the entry commit declares them.

## Bugs surfaced and fixed

- **The doctor Ollama probe could not read a long model list.** Over HTTP/1.1, Ollama sends `/api/tags` with `Transfer-Encoding: chunked`, and the probe parsed the body as plain JSON. `bwoc doctor` therefore reported installed models as missing, and the first pseudo-TTY smoke of bare `bwoc` printed "no model provider configured" with Ollama up. It now sends HTTP/1.0; a regression test uses a local fake server.

## Verification

- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean. Tests for `bwoc-cli`, `bwoc-tui`, `bwoc-harness` and `bwoc-core` pass. `worker::tests::subprocess_runner_kills_on_timeout` failed once in the parallel run and passes single-threaded: the known flake.
- Smoke with the local build, in a temp git repo with `AGENTS.md`:
  - `bwoc < /dev/null` is byte-identical to the pre-change capture, with empty stderr.
  - `script -qfc bwoc` with no Anthropic key and Ollama up held the TUI open until the 30 s timeout (exit 124), where before the probe fix it printed setup help.
  - With `BWOC_ENDPOINT` on a closed port, it printed setup help and exited `2`.
  - `bwoc auth status` printed provider names with "not set".

## Status / deferred

- **R2 / R3:** OAuth is out of scope by decision. Also deferred: a session picker and resume UI; `memory_write` in project sessions (it would write `<cwd>/memories/`); applying the new prompt to batch `bwoc run` / eval; model listing and switching inside the TUI; per-project policy defaults beyond `.bwoc/harness-policy.toml`.
- **Echo during a hidden key read.** Ctrl-C while `bwoc auth set` is reading a hidden key can leave terminal echo off until `reset`.

## Related

- `docs/en/HARNESS.en.md` §Quick start, `docs/en/COMPATIBILITY.en.md` §Versioned artifacts
- PR #519 (R0, `fix/chat-harness-entry`)
