# 2026-06-21 — Deployment HOWTO: non-root agent on a root-only VPS (#322, docs half)

#322: on a **root-only VPS** (Hostinger etc.), the autonomous remote-control
session (`claude --remote-control … --dangerously-skip-permissions`) refuses to
start as root, so there was no documented first-class way to run an autonomous
agent there. The architect chose **both** deliverables: the documented pattern
now (this note), the `bwoc agent run --as-user` helper as a follow-up PR.

## What changed

- New `docs/{en,th}/DEPLOYMENT.md` (nav_order 16) — "Deploying Agents to a
  Server", centered on the **non-root agent user** pattern: create an
  unprivileged service user, relocate + chown the workspace, migrate the agent
  key (owner-only) and backend auth (as the new user, not root), verify with
  `bwoc doctor`, then run the session via `su -` (ad-hoc) or a `User=`-scoped
  systemd unit (supervised). Plus a container note (the existing
  `deploy/standalone-agent.Dockerfile`) and a security checklist.

## Decisions

- **Frame the root refusal as correct, not a bug to bypass.** The vendor CLI's
  "no bypass as root" is the right posture; the doc teaches the non-root user as
  the fix on *every* host, not a root-only workaround. (Sīla over convenience.)
- **Backend auth set up as the service user.** Vendor CLIs store login under the
  running user's home, and API keys belong in the systemd `Environment=`, never
  in the workspace — called out explicitly because it's the easy thing to get
  wrong when migrating from a root setup.
- **Cross-link `bwoc doctor`.** The new #323 key-perms / manifest checks are
  exactly the post-migration verification step, so the doc points at them.

## Status / deferred

- Shipped: the documented pattern (EN/TH). Deferred to PR 2: a
  `bwoc agent run --as-user <user>` helper that automates the privilege-drop +
  launch (chown + `su`/systemd glue). The doc's Roadmap section names it.

## Related

- issue #322; `docs/{en,th}/DEPLOYMENT.md`; `deploy/standalone-agent.Dockerfile`;
  `bwoc doctor` (#323).
