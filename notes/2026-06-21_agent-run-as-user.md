# 2026-06-21 — `bwoc agent run --as-user` helper (#322, helper half)

Second of the two #322 deliverables (docs landed in #325). Automates the
privilege-drop + launch from the deployment HOWTO so an operator doesn't
hand-write the `su`/systemd glue on a root-only VPS.

## What changed

- New `bwoc agent run --as-user <user> <agent> [-- <cmd…>]`
  (`crates/bwoc-cli/src/agent_run.rs`, new `Agent` subcommand group). From root,
  it drops to the target user via `runuser` and launches **in the agent's own
  directory** (so the backend reads `config.manifest.json` / `AGENTS.md` from
  CWD). No `-- <cmd>` → `bwoc-agent --serve`; with one → that command (e.g. a
  remote-control session).
- Updated `docs/en/DEPLOYMENT.en.md` + `docs/th/DEPLOYMENT.th.md` — the "Roadmap" placeholder became a real
  "Helper" section with usage (the helper now exists).

## Decisions (conservative by design — it manages privilege)

- **Unix-only; must run as root.** Privilege-drop is downward-only and POSIX —
  non-Unix and non-root both exit 2 with a clear message.
- **No user creation, no `chown`.** The docs own those one-time steps; a launcher
  silently rewriting ownership would be a footgun. It *warns* (not fails) when
  the agent dir isn't owned by the target user (stat uid vs `id -u <user>`).
- **`runuser`, not hand-rolled setuid.** `runuser -u <user> -- <cmd>` is the
  util-linux, non-interactive-from-root primitive that preserves CWD; we set the
  CWD to the agent dir via `Command::current_dir`. If `runuser` is absent, it
  errors with a hint to the systemd path rather than falling back to fragile
  `su -c` shell-quoting.
- **uid checks via `id -u`**, not a libc dep (keeps `bwoc-cli` lean, matching
  doctor's std-only ethos).

## Verification

4 unit tests (default-vs-explicit launch command, `runuser` argv shape, agent-id
normalization). fmt + clippy clean. Smoke on macOS (non-root): refuses with
"must be run as root to drop to '<user>'" and exit 2; `--help` renders. The live
root→user drop is a Linux-VPS path (not exercisable on the Mac dev host).

## Status / deferred

- #322 complete (docs #325 + this helper). The `su`-fallback path was
  deliberately skipped (requires `runuser`) to avoid shell-quoting hazards.

## Related

- issue #322; `crates/bwoc-cli/src/agent_run.rs`, `crates/bwoc-cli/src/main.rs`;
  `docs/en/DEPLOYMENT.en.md` + `docs/th/DEPLOYMENT.th.md` (#325).
