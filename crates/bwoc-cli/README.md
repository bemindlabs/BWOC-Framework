# bwoc-cli

The `bwoc` command-line tool — incarnate, check, spawn, and control [BWOC](../../README.md) agents.

A single native binary (`bwoc`) for **macOS · Linux · Windows**, and the operator surface for the whole framework: 60 top-level subcommands defined in one clap `Commands` enum in `src/main.rs`. It links the sibling crates [`bwoc-core`](../bwoc-core/) (shared types), [`bwoc-signing`](../bwoc-signing/) (used by `send` / `trust`), [`bwoc-tui`](../bwoc-tui/) (used by `chat`), and [`bwoc-loop-tui`](../bwoc-loop-tui/) (used by `loop`). It does **not** link [`bwoc-harness`](../bwoc-harness/) — `bwoc eval` shells out to the `bwoc-harness` binary, keeping provider/network code out of this crate.

Output is localized (**EN · TH**) through Fluent bundles embedded at compile time from `locales/<lang>/cli.ftl`; adding a language is a file drop plus one match arm in `src/i18n.rs`.

## Scope

| Family | Commands |
|---|---|
| **Lifecycle** (uppāda → ṭhiti → vaya) | `init` · `new` · `spawn` · `start` · `stop` · `retire` · `set` · `debase` |
| **Inspect** | `list` · `status` · `info` · `doctor` · `check` · `workspace` · `sessions` · `log` |
| **Loop-Engineering** | `loop` (L1 goal-loop TUI) · `monitor` (L3, alerts once per OK↔TRIP transition) · `digest` (L3, runs `--exec` once per `--period` via a durable idempotency ledger) |
| **Messaging** | `send` · `inbox` · `outbox` · `triage` · `receipts` · `chat` · `ping` · `a2a` |
| **Saṅgha & fleet** | `team` · `task` · `tasks` · `fleet` · `peer` · `trust` · `supervise` · `remote` |
| **Docs & memory** | `notes` · `retro` · `research` · `doc` · `memory` |
| **Extensions** | `skill` · `plugin` · `audit` · `resource`, plus the plugin-kind fronts `jira` · `gcloud` · `okr` · `council` · `figma` · `gws` · `accounting` (live verbs exit `4` with no installed plugin of that kind) |
| **Ergonomics** | `help` · `handbook` · `dashboard` · `completion` · `update` · `report` · `eval` · `run` · `agent` |

Arc phases are named per [`PHILOSOPHY.en.md` §0.1](../../modules/agent-template/docs/en/PHILOSOPHY.en.md). Loop levels are specified in [`LOOP-ENGINEERING.en.md`](../../docs/en/LOOP-ENGINEERING.en.md); extension surfaces in [`SKILLS.en.md`](../../docs/en/SKILLS.en.md) and [`PLUGINS.en.md`](../../docs/en/PLUGINS.en.md).

## Install

```bash
./scripts/install.sh                          # builds bwoc, bwoc-agent, bwoc-harness
cargo install --path crates/bwoc-cli --locked # this binary only
```

Both land in `~/.cargo/bin/`. Requires a [Rust toolchain](https://rustup.rs/) on PATH.

## Usage

```bash
bwoc --help                       # full command surface
bwoc init .                       # create a workspace
bwoc new tara --template modules/agent-template --target agents/agent-tara
bwoc check --all                  # backend-neutrality audit
bwoc loop --team core             # goal-loop control center
bwoc completion zsh > ~/.zfunc/_bwoc
```

### Workspace resolution

Workspace-aware commands take `--workspace <path>`, then `BWOC_WORKSPACE`, then walk ancestors for `.bwoc/workspace.toml`; if none is found they exit `2` with an actionable message. See [`WORKSPACE.en.md`](../../docs/en/WORKSPACE.en.md).

### Language

`--lang <code>` → `BWOC_LANG` → `$LANG` (POSIX values like `th_TH.UTF-8` are parsed) → `en` fallback.

```bash
BWOC_LANG=th bwoc list
```

## Status

Shipping and in daily use. All the families above are implemented. Two surfaces are still partial: `bwoc a2a serve` binds loopback by default — a non-loopback bind requires a Bearer token (`BWOC_A2A_TOKEN` or `.bwoc/a2a.token`) or an explicit `--allow-unauthenticated`; and `bwoc resource` ships `snapshot` · `gate-check` (local) plus `advertise` · `discover` (via the gateway broker), with `claim` still to land.

## License

[MIT](../../LICENSE).
