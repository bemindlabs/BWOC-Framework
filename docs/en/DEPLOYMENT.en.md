---
title: Deployment
parent: English
nav_order: 16
---

# Deploying Agents to a Server

This guide covers running a BWOC agent as a long-lived session on a remote
host — in particular the common **root-only VPS** case (Hostinger, many cheap
providers) where you log in as `root` and no unprivileged user exists yet.

> [!warning] **Autonomous agents must not run as `root`.** Vendor CLIs refuse the autonomous (bypass-permission) mode under root for exactly this reason — for example `claude --remote-control <name> --dangerously-skip-permissions` exits with *"cannot be used with root/sudo privileges for security reasons."* The fix is not to force it; it is to run the agent as a dedicated unprivileged user. That is the recommended posture on **every** host, not just root-only ones.

---

## The non-root agent user

Create one unprivileged service user that owns the agent workspace and runs the
session. Five steps, run once as `root`.

### 1. Create the user

```bash
useradd -m -s /bin/bash bwoc          # -m = home dir; pick any name
# (Debian/Ubuntu alternative: adduser --disabled-password --gecos "" bwoc)
```

### 2. Move the workspace under the new user

If you already incarnated the agent as root, relocate its workspace into the
user's home and hand over ownership:

```bash
mv /root/my-workspace /home/bwoc/      # the dir holding .bwoc/ + agents/
chown -R bwoc:bwoc /home/bwoc/my-workspace
```

Starting fresh instead? Just run `bwoc init` / `bwoc new` **as the `bwoc` user**
(see step 5 for how to become it) so everything is owned correctly from the
start.

### 3. Migrate credentials

The agent's own identity and any backend auth must be readable by the new user:

- **Agent signing key** — `agents/<agent>/.bwoc/agent.key`. It moves with the
  workspace in step 2; confirm it is **owner-only** afterward:
  ```bash
  chmod 600 /home/bwoc/my-workspace/agents/<agent>/.bwoc/agent.key
  ```
  (`bwoc doctor` flags a group/other-readable key, and `bwoc doctor --auto`
  chmods it.)
- **Backend auth** — set this up **as the `bwoc` user**, not root, because the
  vendor CLIs store their session/login under the *running user's* home:
  - Subscription/login CLIs (Claude, Codex): run the CLI's own login once as
    `bwoc` (e.g. `claude login`), so the token lands in `~bwoc/`.
  - API-key backends: put the key in the `bwoc` user's environment (the systemd
    unit's `Environment=` in step 5, or its shell profile) — never in the
    workspace or a world-readable file.

### 4. Verify as the new user

```bash
su - bwoc
cd ~/my-workspace
bwoc doctor            # manifests, symlinks, key perms, model availability
bwoc list              # the agent shows up, owned by bwoc
```

### 5. Run the session — pick one

**Ad-hoc** (a quick interactive/remote-control session):

```bash
su - bwoc -c 'cd ~/my-workspace/agents/<agent> && claude --remote-control <agent> --dangerously-skip-permissions'
```

Run from the **agent's own directory** (`agents/<agent>/`) — that is where
`AGENTS.md` + `config.manifest.json` live, so the backend loads the agent's
persona/context. Because the command now runs as `bwoc` (not root), the
bypass-permission mode is accepted.

**Supervised** (survives logout/reboot — recommended for a steady worker). A
systemd unit at `/etc/systemd/system/bwoc-agent@.service`:

```ini
[Unit]
Description=BWOC agent %i
After=network-online.target

# `%i` is the agent's directory name under agents/ (after the @ when you enable
# the unit) — `bwoc-agent --serve` reads config.manifest.json from its CWD, so
# the working directory must be the agent dir, not the workspace root.
[Service]
User=bwoc
Group=bwoc
WorkingDirectory=/home/bwoc/my-workspace/agents/%i
# API-key backends only — subscription CLIs use the bwoc user's stored login:
# Environment=ANTHROPIC_API_KEY=...
ExecStart=/usr/local/bin/bwoc-agent --serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload
systemctl enable --now bwoc-agent@<agent>.service
journalctl -u bwoc-agent@<agent> -f        # follow its logs
```

`User=bwoc` is what makes this safe: systemd drops to the unprivileged user, so
the daemon (and any harness session it supervises) never holds root.

---

## Containers (an alternative isolation)

A container is another way to get a non-root, isolated runtime — the repo ships
[`deploy/standalone-agent.Dockerfile`](https://github.com/bemindlabs/BWOC-Framework/blob/main/deploy/standalone-agent.Dockerfile)
as a starting point. Run the container as a non-root `USER` and mount the
workspace; the same credential rules apply (key owner-only, backend auth in the
container's environment, not baked into the image).

---

## Security checklist

- [ ] Agent session runs as an **unprivileged user**, never `root`.
- [ ] `agent.key` is **owner-only** (`chmod 600`) — verify with `bwoc doctor`.
- [ ] Backend API keys live in the service environment, **not** in the workspace
      or a world-readable file.
- [ ] The unprivileged user has **no `sudo`** rights it does not need.

---

## Roadmap

A `bwoc agent run --as-user <user>` helper to automate the privilege-drop +
launch (so you don't hand-write the `su`/systemd glue) is planned as a
follow-up. Until then, this documented pattern is the supported path.

---

## Related

- [[INCARNATION]] — creating the agent in the first place
- [[WORKSPACE]] — what `bwoc init` lays down (the dir you relocate in step 2)
- `bwoc doctor` — validates key perms, manifests, and model availability on the host
