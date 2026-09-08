# Security Policy

## Supported Versions

**The latest release only.** Fixes land on `main` and ship in the next release;
there are no maintenance branches to backport onto — tags are cut directly on
`main`, and `CONTRIBUTING.md` forbids `release/*` branches. A security fix is a
reason to cut a release, not to open a support line for an older one.

If you are running an older release, upgrading is the fix. See
[`docs/en/COMPATIBILITY.en.md`](docs/en/COMPATIBILITY.en.md) for what an upgrade
can and cannot change under you, and
[`docs/en/MIGRATION.en.md`](docs/en/MIGRATION.en.md) for the 2.x → 3.0
step.

## Reporting a Vulnerability

**Do not open a public issue or pull request for security vulnerabilities.**

Email **info@bemind.tech** with:

- A description of the issue and the impact you observed.
- Steps to reproduce (proof-of-concept, minimal repro repo, or commands).
- Affected version (commit SHA or release tag).
- Any suggested mitigation, if known.

Acknowledgement, triage, remediation, and disclosure timelines are negotiated case-by-case with the reporter. The project does not currently publish a fixed-window service-level commitment.

## Disclosure

We follow **coordinated disclosure**. Once a fix is available, we will:

1. Publish a patched release.
2. Credit the reporter (unless anonymity is requested).
3. Open a public advisory describing the issue, impact, and remediation.

## Scope

In scope:

- The framework specification documents (`docs/`, `modules/agent-template/`).
- Scripts in `modules/agent-template/scripts/` and any future first-party tooling.
- Configuration schemas (`config.manifest.json`).
- Hooks and skills shipped under `.claude/`.

Out of scope:

- Vulnerabilities in third-party LLM backends (Claude, Antigravity, Codex, Kimi, etc.) — report those to the respective vendor.
- Issues in agents incarnated from this template and modified downstream — report to that agent's maintainer.
- Social-engineering or physical attacks against contributors.

## Threat Model

The full threat model — including prompt injection, capability spoofing, persistence, and destruction vectors — is documented in [`modules/agent-template/docs/en/THREAT-MODEL.en.md`](modules/agent-template/docs/en/THREAT-MODEL.en.md), structured by Taṇhā 3 (Three Cravings) and Sīla 5 (Five Precepts).

## Baseline Security Rules

These are enforced by convention in every incarnated agent:

- No `rm -rf` of repo root.
- No committing secrets, credentials, or personal identifiers.
- No spoofing agent identity.
- No bypassing verification gates.
- No undeclared side-effects outside the declared task scope.
