# Contributing to BWOC Framework

Thank you for considering a contribution. BWOC is a framework for building AI coding agents grounded in Buddhist philosophy as an engineering discipline — contributions should be technical, kind, and verifiable.

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## How You Can Contribute

- **Report a bug** — open an issue using the *Bug report* template.
- **Suggest an enhancement** — open an issue using the *Feature request* template.
- **Improve documentation** — typos, clarifications, additional examples, bilingual (English/Thai) parity.
- **Add or refine a framework mapping** — see the 22 frameworks in [README.md](README.md).
- **Add a reference agent** — see [`modules/agent-template/`](modules/agent-template/).

---

## Development Workflow

We use a single **trunk-based** branching standard across every BWOC repo: `main` is the only long-lived branch and is always releasable; all work happens on short-lived topic branches that are deleted after merge. The canonical naming rules live in [`modules/agent-template/conventions.md`](modules/agent-template/conventions.md#branch-names-typetask-id).

1. **Fork** the repository and create a topic branch from `main`. Branch `type` uses the same vocabulary as commit types (`feat fix docs refactor test chore perf style ci`):
   ```bash
   git checkout -b feat/<short-name>     # for features
   git checkout -b fix/<short-name>      # for bug fixes
   git checkout -b docs/<short-name>     # for documentation
   ```
   No `release/*` or `hotfix/*` branches — version tags (CalVer `v<YYYY>.<M>.<D>-<patch>`) are cut directly on `main`.
2. **Make your changes** — keep diffs focused. One concern per PR.
3. **Verify** before committing:
   - Documentation renders correctly (Markdown lint, link check).
   - Examples actually work when copy-pasted.
   - English and Thai docs stay in pair when both are touched.
4. **Commit** with a descriptive message — see [Commit Style](#commit-style) below.
5. **Open a Pull Request** against `main` using the PR template.

---

## Commit Style

We follow a lightweight [Conventional Commits](https://www.conventionalcommits.org/) style:

```
<type>(<scope>): <short summary>

[optional body — explain *why*, not *what*]
[optional footer — refs/closes #issue]
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`, `ci`.

**Examples:**
- `docs(philosophy): clarify Yoniso Manasikāra example`
- `feat(agent-template): add neutrality check hook`
- `fix(srs): correct cross-reference to Magga 8`

---

## Pull Request Checklist

Before requesting review, ensure:

- [ ] Branch is up to date with `main`
- [ ] PR title follows commit style
- [ ] Description explains *why* the change is needed
- [ ] Related issues are linked (`Closes #123`)
- [ ] Documentation is updated where applicable
- [ ] Bilingual EN/TH pair preserved (if applicable)
- [ ] No secrets, credentials, or personal paths committed

---

## Merging & External PRs

**`main` is a protected branch.** Changes land **only** through a pull request
that GitHub auto-merges once its gates are green — never by a direct push,
including automation (the release pipeline's Homebrew-formula bump opens its own
auto-merged PR rather than pushing to `main`).

A PR is mergeable only when **all** of these hold:

- The four required CI checks pass: **fmt + clippy** and **build + test** on
  macOS, Linux, and Windows.
- The branch is **up to date with `main`** — strict protection requires it to
  contain the latest `main`, so a conflict-free but stale branch must still be
  updated (`gh pr update-branch <pr>`) before it can merge.
- **Every review conversation is resolved** — a requested change or open thread
  blocks the merge until it's addressed.

Enable native auto-merge with `gh pr merge <pr> --squash --auto`; GitHub merges
the moment the gates clear.

### External / fork PRs

Cross-repo (fork) PRs carry external code into a security framework, so they get
a **full review before merge — never a blind merge**:

- Correctness + **security** (any code that executes plugins/skills/scripts, or
  parses untrusted input, is scrutinized for injection / path-traversal / RCE).
- **Dep-quarantine** held (`bwoc-core` stays lean; crypto/heavy deps only where
  the rule allows).
- **Backend neutrality** of `AGENTS.md` and **EN/TH bilingual parity** intact.
- Rebased conflict-free, CI green, and **no unresolved blocking findings**.

If a fork PR has security or correctness blockers, request changes (it stays
blocked by the conversation-resolution gate) rather than merging "to fix later."

---

## Documentation Principles (BWOC-aligned)

When writing or reviewing docs, apply these principles from the framework itself:

1. **Yoniso Manasikāra** — verify references against the current code/state, not from memory.
2. **Mattaññutā (Right Amount)** — concise is kind. If `MEMORY.md` is capped at 200 lines, your section probably can be too.
3. **Samānattatā** — treat all LLM backends equally. No vendor-specific phrasing in core docs.
4. **Sīla** — never include secrets, internal hostnames, or personal identifiers in examples.

---

## Reporting Security Issues

**Do not open public issues for security vulnerabilities.** See [SECURITY.md](SECURITY.md) for the private disclosure process.

---

## Questions

- General questions → open a [Discussion](https://github.com/bmt-bwol-ops/bwoc-framwork/discussions) (or an issue tagged `question` if discussions are not enabled).
- Sensitive matters → email **info@bemind.tech**.

---

## Recognition

Contributors are credited in commit history and release notes. Significant contributors may be invited to the maintainer team — see [CODEOWNERS](.github/CODEOWNERS).

Thank you for helping the framework grow.
