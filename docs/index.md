---
title: Home
nav_order: 1
permalink: /
---

# BWOC Framework — Documentation

**Buddhist Way of Coding** — a backend-neutral specification for AI coding agents, plus the native Rust implementation that incarnates and runs them.

Pali terms are section names; the content is technical. No religious interpretation required.

---

## English (canonical)

- [Architecture](en/ARCHITECTURE.en.md) — how the CLI, workspace, agents, and runtime fit together
- [Incarnation](en/INCARNATION.en.md) — step-by-step agent creation
- [Workspace](en/WORKSPACE.en.md) — `.bwoc/` layout, validation rules, resolution precedence
- [Naming](en/NAMING.en.md) — the 12-category `*.md` naming standard
- [Glossary](en/GLOSSARY.en.md) — Pali term lookup
- [Roadmap](en/ROADMAP.en.md) — phase-by-phase plan
- [Threat Model](en/THREAT-MODEL.en.md) — the turn-executor trust boundary (Phase 5)
- [FAQ](en/FAQ.en.md) — newcomer questions

## ภาษาไทย (Thai)

- [สถาปัตยกรรม](th/ARCHITECTURE.th.md)
- [การถือกำเนิด](th/INCARNATION.th.md)
- [Workspace](th/WORKSPACE.th.md)
- [การตั้งชื่อ](th/NAMING.th.md)
- [อภิธานศัพท์](th/GLOSSARY.th.md)
- [แผนพัฒนา](th/ROADMAP.th.md)
- [แบบจำลองภัยคุกคาม](th/THREAT-MODEL.th.md)
- [คำถามที่พบบ่อย](th/FAQ.th.md)

---

## Template-level docs

The cloneable agent template ships with its own doctrine layer (Philosophy, PRD, SRS, Self-Improvement, Threat Model). These live inside the template and are read by every incarnated agent:

- [`modules/agent-template/docs/en/`](https://github.com/bemindlabs/BWOC-Framework/tree/main/modules/agent-template/docs/en)
- [`modules/agent-template/docs/th/`](https://github.com/bemindlabs/BWOC-Framework/tree/main/modules/agent-template/docs/th)

---

## Project resources

- [README](https://github.com/bemindlabs/BWOC-Framework#readme) — start here
- [VISION](https://github.com/bemindlabs/BWOC-Framework/blob/main/VISION.md) — 1-year and 3-year success criteria
- [CHANGELOG](https://github.com/bemindlabs/BWOC-Framework/blob/main/CHANGELOG.md) — what shipped, when
- [GitHub Releases](https://github.com/bemindlabs/BWOC-Framework/releases) — binaries + SHA-256 checksums
