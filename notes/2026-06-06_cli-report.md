# 2026-06-06 — CLI: `bwoc report` (user issue → GitHub issue)

Third of the CLI self-service batch (after `bwoc handbook` + `bwoc info`).
Lets a user file a bug/feature/question against the framework repo without
leaving the terminal.

## What changed

- **`report.rs`** — `bwoc report "<title>" [--body …] [--kind bug|feature|
  question] [--web] [--yes]`. Preview → confirm → `gh issue create --repo
  bemindlabs/BWOC-Framework`. The body always appends an **Environment** block
  (version, release identity, OS/arch). `--kind feature` maps to the stock
  `enhancement` label (GitHub has no default `feature`).
- **Fail-safe**: `--web`, unauthenticated/missing `gh`, or non-TTY without
  `--yes` → print a prefilled `issues/new?title=…&body=…&labels=…` URL instead.
  A public issue is never filed unattended (mirrors the harness's ask→deny).
- `update::GITHUB_REPO` made `pub(crate)` and reused; shell-outs go through the
  existing `ShellRunner` seam (no new deps), `run_inner(args, runner, tty)` is
  the testable core.

## Tests

9 unit tests: percent-encoding (space/UTF-8 Thai/unreserved), env block,
kind mapping + rejection, usage errors make no shell-outs, `--web` never
creates, non-TTY-without-`--yes` never creates, unauthenticated `gh` falls
back, create path passes repo/title/body/label, failed create exits 1 with the
URL fallback. Smoke: `--web` prints a correctly-encoded URL (Thai title).

## Status

Done pending CI. CLI self-service batch complete; next per the queue:
Telegram/Discord chat connectors on the bwoc-agent daemon (design note first).

## Related

- `crates/bwoc-cli/src/{report,update,main}.rs`
- `notes/2026-06-06_cli-handbook-info.md` (siblings)
