#!/usr/bin/env bash
# install.sh — one-command install (or upgrade) of the bwoc toolkit.
#
# Builds and installs ALL THREE binaries to ~/.cargo/bin/:
#   bwoc         — the CLI (init, new, list, start, stop, status, etc.)
#   bwoc-agent   — the per-agent daemon (spawned by `bwoc start`)
#   bwoc-harness — the agentic loop (spawned for chat --tui / eval / --headless)
#
# The agent template is embedded in the `bwoc` binary at compile time,
# so the installed CLI works from any directory — no on-disk template
# required.
#
# Requires: a Rust toolchain (https://rustup.rs/).
#
# Usage (from a clone of bwoc-framework):
#   ./scripts/install.sh                # install or upgrade in place (--force)
#   ./scripts/install.sh --check        # report current install state, no build
#   ./scripts/install.sh --uninstall    # remove all three binaries
#   ./scripts/install.sh --help         # this message
#
# Or one-liner from the repo root (CLI only — won't get bwoc-agent/bwoc-harness):
#   cargo install --path crates/bwoc-cli --locked --force
#
# Related:
#   ./scripts/bump-version.sh           # manual major/minor/patch SemVer bumps
#                                       # (patch also auto-bumped on every edit
#                                       # by .claude/hooks/auto-version.sh)

set -euo pipefail

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  sed -n '2,26p' "$0" | sed 's/^# \?//'
  exit 0
fi

if [ "${1:-}" = "--check" ]; then
  echo "bwoc toolkit install state:"
  if command -v bwoc >/dev/null 2>&1; then
    echo "  bwoc:       $(command -v bwoc) — $(bwoc --version 2>/dev/null | head -1)"
  else
    echo "  bwoc:       not installed"
  fi
  if command -v bwoc-agent >/dev/null 2>&1; then
    echo "  bwoc-agent: $(command -v bwoc-agent) — present (no --version flag)"
  else
    echo "  bwoc-agent: not installed"
  fi
  if command -v bwoc-harness >/dev/null 2>&1; then
    echo "  bwoc-harness: $(command -v bwoc-harness) — present"
  else
    echo "  bwoc-harness: not installed (chat --tui / eval / --headless will fail)"
  fi
  case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) echo "  PATH:       \$HOME/.cargo/bin is on PATH ✓" ;;
    *) echo "  PATH:       \$HOME/.cargo/bin is NOT on PATH (binaries hidden)" ;;
  esac
  exit 0
fi

if [ "${1:-}" = "--uninstall" ]; then
  echo "Uninstalling bwoc toolkit ..."
  removed=0
  if command -v bwoc >/dev/null 2>&1; then
    cargo uninstall bwoc-cli 2>&1 | sed 's/^/  /' || true
    removed=1
  else
    echo "  bwoc:       not installed (skipped)"
  fi
  if command -v bwoc-agent >/dev/null 2>&1; then
    cargo uninstall bwoc-agent 2>&1 | sed 's/^/  /' || true
    removed=1
  else
    echo "  bwoc-agent: not installed (skipped)"
  fi
  if command -v bwoc-harness >/dev/null 2>&1; then
    cargo uninstall bwoc-harness 2>&1 | sed 's/^/  /' || true
    removed=1
  else
    echo "  bwoc-harness: not installed (skipped)"
  fi
  if [ "$removed" = "0" ]; then
    echo ""
    echo "Nothing was installed — nothing to uninstall."
  else
    echo ""
    echo "Uninstall complete."
    echo "Note: per-workspace state (.bwoc/ dirs) is NOT removed by this script."
    echo "      Use \`bwoc retire\` per agent or rm -rf the workspace yourself."
  fi
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: 'cargo' not found on PATH. Install Rust from https://rustup.rs/ first." >&2
  exit 1
fi

# Pre-flight: warn if ~/.cargo/bin isn't on PATH — that's where cargo
# install puts the binaries, so it must be on PATH for them to be
# discoverable. Don't fail; the user might have a custom shell setup.
case ":$PATH:" in
  *":$HOME/.cargo/bin:"*) ;;
  *)
    echo "warning: \$HOME/.cargo/bin is not on PATH." >&2
    echo "         The binaries will install but won't be discoverable." >&2
    echo "         Add this to your shell config (~/.bashrc, ~/.zshrc, etc.):" >&2
    echo "           export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
    echo "" >&2
    ;;
esac

# Detect existing installs so we can phrase the message accurately.
existing_cli=""
if command -v bwoc >/dev/null 2>&1; then
  existing_cli="$(bwoc --version 2>/dev/null | head -1 || true)"
fi
existing_agent=""
if command -v bwoc-agent >/dev/null 2>&1; then
  # bwoc-agent doesn't accept --version; treat presence as enough.
  existing_agent="bwoc-agent"
fi

if [ -n "$existing_cli" ] || [ -n "$existing_agent" ]; then
  echo "Upgrading bwoc toolkit in place from $REPO_ROOT ..."
  [ -n "$existing_cli" ] && echo "  bwoc:       was $existing_cli"
  [ -n "$existing_agent" ] && echo "  bwoc-agent: was present"
else
  echo "Installing bwoc toolkit from $REPO_ROOT ..."
fi
echo ""

# 1. The CLI.
echo "[1/3] cargo install bwoc-cli ..."
cargo install --path crates/bwoc-cli --locked --force

# 2. The daemon binary — required for `bwoc start`'s daemon spawn,
# `bwoc-agent --serve`, and the PING/STATUS/STOP IPC protocol.
echo ""
echo "[2/3] cargo install bwoc-agent ..."
cargo install --path crates/bwoc-agent --locked --force

# 3. The harness binary — runs the agentic loop. `bwoc` spawns it by name, so
# without it `bwoc chat --tui`, `bwoc eval`, `--headless`, `--lead` and `--task`
# all fail at spawn (issue #460).
echo ""
echo "[3/3] cargo install bwoc-harness ..."
cargo install --path crates/bwoc-harness --locked --force

echo ""
new_version="$(bwoc --version 2>/dev/null | head -1 || echo 'bwoc (not on PATH)')"
echo "Installed: $new_version"
if command -v bwoc-agent >/dev/null 2>&1; then
  echo "Installed: bwoc-agent (daemon)"
else
  echo "warning: bwoc-agent installed but not on PATH — see warning above"
fi
if command -v bwoc-harness >/dev/null 2>&1; then
  echo "Installed: bwoc-harness (agentic loop)"
else
  echo "warning: bwoc-harness installed but not on PATH — see warning above"
fi
echo ""
echo "Verify with:"
echo "  bwoc --help"
echo "  bwoc help getting-started"
echo ""
echo "Quickstart:"
echo "  mkdir my-workspace && cd my-workspace"
echo "  bwoc init"
echo "  bwoc new alpha            # interactive picker for backend / role / model"
echo "  bwoc start alpha          # spawns bwoc-agent --serve in the agent's dir"
echo "  bwoc list                 # see runtime + inbox count"
echo ""
