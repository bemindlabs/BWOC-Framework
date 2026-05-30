# How-To: Your First Agent in 5 Minutes

## Goal

A working BWOC workspace with one incarnated agent, ready for `bwoc spawn`.

## Prerequisites

- `bwoc` CLI installed (`./scripts/install.sh` from a clone, or `cargo install --path crates/bwoc-cli --locked --force`)
- At least one backend CLI on PATH (`claude`, `agy`, `codex`, or `kimi`) if you want to actually spawn the backend; not required for incarnation itself
- An empty directory you can write to

## Steps

### 1. Initialize a workspace

```bash
mkdir my-workspace && cd my-workspace
bwoc init
```

Expected output:

```
Initialized BWOC workspace at: /…/my-workspace
+ .bwoc/workspace.toml
+ .bwoc/agents.toml
+ agents/   (incarnated agents land here)
+ projects/ (your work — apps/repos the agents help build)
+ notes/    (implementation logs — YYYY-MM-DD_<title>.md)
```

Each scaffold dir now has a `README.md` explaining its purpose.

### 2. Incarnate your first agent

```bash
bwoc new alpha
```

You'll be prompted for required manifest fields. **Press Enter to accept defaults** (defaults are stack-detected from your cwd — Rust shows `cargo` commands, Node shows `npm`, etc.). `primaryModel` shows a numbered picker per backend:

```
agentRole (Short role description for this agent): documentation writer
Common claude models (pick a number, or type a custom model name):
  1. claude-opus-4-8  (default: 1)
  2. claude-sonnet-4-6
  3. claude-haiku-4-5
primaryModel (Primary LLM model identifier): ↵
lintCmd (Lint command) [default: cargo clippy --all-targets -- -D warnings]: ↵
formatCmd (Format check command) [default: cargo fmt --all -- --check]: ↵
testCmd (Test command) [default: cargo test --workspace]: ↵
buildCmd (Build command) [default: cargo build --workspace]: ↵
```

Expected report:

```
Incarnated agent: agent-alpha
Target:           /…/my-workspace/agents/agent-alpha

+ CLAUDE.md -> AGENTS.md
+ AGY.md -> AGENTS.md
+ CODEX.md -> AGENTS.md
+ KIMI.md -> AGENTS.md

Registered in workspace: /…/my-workspace (appended to .bwoc/agents.toml)

Next steps:
  1. cd /…/my-workspace/agents/agent-alpha && bwoc check .
  2. Edit AGENTS.md Section 1 — fill {{placeholders}} that aren't manifest fields.
  3. Edit persona/README.md — define identity, domains, boundaries.
  4. git init && git add -A && git commit -m 'feat(agent): incarnate'
```

### 3. Verify the agent is backend-neutral

```bash
bwoc check agents/agent-alpha
```

Expected: 15 PASS lines (one per neutrality rule) followed by `Neutrality check passed.`

### 4. List what's registered

```bash
bwoc list
```

Expected:

```
ID                               STATUS     BACKEND    PATH
──────────────────────────────── ────────── ────────── ────────────────────
agent-alpha                      active     claude     agents/agent-alpha
```

### 5. Spawn the backend (optional)

```bash
bwoc spawn --path agents/agent-alpha --backend claude
```

This `exec`s the `claude` CLI in the agent's directory. The backend reads `AGENTS.md` (or its `CLAUDE.md` symlink) and starts a session aware of the agent's persona, memories, and skills slots.

## Verify

```bash
bwoc doctor
```

Should report all PASS with `5 pass · 0 warn · 0 fail · 0 fixed`. If anything's broken, `bwoc doctor --auto` fixes safe issues automatically.

## What's next

- Open `agents/agent-alpha/persona/README.md` and define the agent's identity, domains, and boundaries.
- Read the rest of `agents/agent-alpha/AGENTS.md` Section 1 — it's the agent's "constitution."
- See [`workspace-layout.md`](workspace-layout.md) to understand where everything lives.
- See [`configure-backends.md`](configure-backends.md) to switch which LLM backend an agent uses.
- For the formal spec, read [`docs/en/INCARNATION.en.md`](../../docs/en/INCARNATION.en.md).
