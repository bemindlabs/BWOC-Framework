# 2026-08-19 — `bwoc run` headless dispatch for the `codex` backend

`bwoc run` returned `HeadlessUnsupported` for `codex` on the grounds that "no confirmed
non-interactive exec flag exists". That was true when the arm was written; it is no longer
true — codex-cli ships `codex exec` ("Run Codex non-interactively") with approval policy
`never`. The deferral was closed against the real CLI rather than against the docs.

## What changed

- `crates/bwoc-cli/src/run.rs` — `Backend::Codex` now builds
  `codex exec --skip-git-repo-check --color never --sandbox workspace-write -- "<task>"`
  instead of erroring. Module-level dispatch table and the stdin contract updated.
- Same file — `ProcessCommandRunner` now sets `.stdin(Stdio::null())` for **every** backend.
- Tests — the `codex_returns_headless_unsupported` assertion is replaced by four:
  exact argv, no model forwarded, `--` keeps a dash-leading task positional, and the
  bypass flag stays absent. `headless_unsupported_propagated_from_execute` moved to `kimi`.

## Decisions

**`--sandbox workspace-write`, not the default.** `codex exec` defaults to read-only, which
cannot complete an edit task — the whole point of headless dispatch. `workspace-write`
bounds writes to the run cwd, which is exactly the jail `resolve_workdir` already
canonicalizes and validates. The blast radius is unchanged; only the permission to act
inside it is granted. `--dangerously-bypass-approvals-and-sandbox` stays off, mirroring the
existing refusal of copilot's `--allow-all-tools` (Sīla — the fail-safe default is the rule,
not the exception).

**No `-m <primaryModel>`.** The obvious symmetry with the harness backends is wrong here.
A manifest's `primaryModel` is backend-neutral by design, and vendor CLIs resolve their own
model from their own config plus account entitlement. This is not hypothetical: the live
`agent-caoguojiu` pairs `backend = "codex"` with `primaryModel = "claude-sonnet-4-6"`, and
`codex exec -m claude-sonnet-4-6` is a hard 400. The `claude` arm already sets this
precedent — it forwards `--effort` but never `--model`. Forwarding a neutral value into a
vendor namespace breaks Samānattatā in the direction that costs a working run.

**`--` before the task.** The prompt is positional; a task beginning with `-` would
otherwise be parsed as a flag. Cheap, and the failure mode it prevents is silent.

**stdin null for all backends, not just codex.** Scoping it to codex would leave the same
hang latent on `claude -p` and `copilot -p`. "Headless" is a property of `bwoc run`, not of
one backend, so the contract belongs on the shared runner.

## Alternatives considered

- **Forward `reasoningEffort` as `-c model_reasoning_effort=<level>`**, mirroring the claude
  arm's `--effort`. Rejected: the two CLIs do not share a level vocabulary (claude has
  `xhigh`/`max`, codex does not), so the mapping would be a guess. Mattaññutā — left out
  until someone needs it.
- **Leave the sandbox at codex's default** and let operators set it in
  `$CODEX_HOME/config.toml`. Rejected: `bwoc run` would then behave differently per machine
  for the same agent, which is precisely the non-determinism headless orchestration exists
  to remove.

## Status / verification

- `cargo fmt` clean · `cargo clippy -p bwoc-cli --all-targets -D warnings` clean ·
  `cargo test -p bwoc-cli` 900 passed.
- **Live dispatch verified, model call not.** `bwoc run caoguojiu --task "…"` in
  `~/workspaces/bwoc` launched `codex exec` in the agent directory, resolved the model from
  codex config (`gpt-5.6-luna` — confirming the no-`-m` decision), ran with
  `approval: never` / `sandbox: workspace-write`, and captured output plus exit code back
  into `RunResult` in 6.5 s. The run itself returned exit 1 because the ChatGPT account is
  **over its usage limit until 2026-08-23**. The plumbing is proven end-to-end; a green
  agent turn on this backend has not been observed and should be re-run after the quota
  resets.
- `agy` and `kimi` remain `HeadlessUnsupported` — out of scope here (one concern per PR).
