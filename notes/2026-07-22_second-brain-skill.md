# 2026-07-22 — `second-brain` + `server-rag` framework skills (the fleet's two memories)

Adds two sibling framework skills that encode "consult the fleet's existing knowledge before re-deriving" — Remember-first / Yoniso Manasikāra applied *across the whole fleet*, not just the local repo. They cover the fleet's **two** knowledge systems:

- **`second-brain`** — the *structured graph* (query by term across every workspace/repo/commit/PR/issue/memory/note/agent/domain/container).
- **`server-rag`** — the *semantic RAG* (ask a natural-language ops question, get prose + sources).

Sourced from probing the bmt fleet's **Second Brain** (a Vite SPA at `…:30600`) + verifying the self-hosted RAG endpoint.

## What changed

- New `modules/skills/second-brain/` (`manifest.toml` + `SPEC.md`): exposes `query_brain` (read-only graph search by term, ranked by node `degree`) + `refresh_brain` (re-harvest). `domain/knowledge`, L1.
- New `modules/skills/server-rag/` (`manifest.toml` + `SPEC.md`): exposes `ask_rag` (POST `{question, top_k}` → `{answer, sources}`) + `refresh_rag` (re-ingest). `domain/knowledge`, L1. Cross-linked with second-brain ("graph vs RAG — query structure vs ask prose").
- Both show in `bwoc skill list` and pass `bwoc skill verify` (manifest valid, gate printed).

## Decisions

- **Framework skill, not a per-agent slot skill.** The pattern (query a fleet-wide harvested knowledge graph) is reusable across agents and even across BWOC installs, so it belongs in `modules/skills/` (enable-per-agent) rather than one agent's `skills/`.
- **`<secondBrainRoot>` is operator-configured, no hardcoded host/URL** — keeps the skill backend-/environment-neutral (Samānattatā) so `bwoc check` stays happy. The SPEC teaches the *pattern* (graph.json = `{nodes[],links[]}`, node = `{id,label,group,kind,description,degree}`, kinds mem/note/commit/repo/agent/branch/tag/domain/pr/issue/ws/dock), not a specific fleet's address.
- **`query_brain` is a free read; `refresh_brain` touches only the local `graph.json` artifact** → no operator-confirm gate (same reasoning as `okr track`'s local write).
- **`jq` over `graph.json` is the backend-neutral query baseline** (the web UI is the human path). On zero matches: refresh then retry before concluding "no prior art."

## How the source was gathered

Probed the fleet's Second Brain (`…:30600`, a Vite SPA); found it is a static data app — the whole dataset is `brain-web/src/data/graph.json` (harvested by `scripts/harvest.mjs`, `npm run harvest`), no backend API. Verified the graph shape (2768 nodes / 3536 links, node kinds above) and a working `jq` query recipe against it, then generalised that into the skill.

**Yoniso Manasikāra caught a stale fact:** a fleet memory documented the RAG at `localhost:8088`, but that port is dead — the service moved to `:10110` in the "internal ≥ 10000, bind 127.0.0.1" migration. Verified `:10110` live (`POST /query {question, top_k}` → `{answer, sources}`) before writing `server-rag`, and the SPEC deliberately warns against memorised ports (resolve `<ragQueryUrl>` from current infra). **`accounting-api`** was considered as a third related skill but skipped — it *writes* to an external system of record (needs credentials + a confirm gate), so it belongs as a `plugin` kind (like `gws`/`jira`), not a knowledge-retrieval skill.

## Status / deferred

- Shipped the skill at L1. Bumps to L2 once two agents use `query_brain` to reuse prior fleet knowledge end-to-end; L3 once the verify gate is wired + green in CI.
- Not enabled on any agent yet — `bwoc skill enable second-brain` on the agents that should consult the fleet brain (e.g. orchestrators) is an operator step.

## Related (links)

- `modules/skills/second-brain/`; `docs/en/SKILLS.en.md` (the spec it conforms to).
- Pattern kin: `modules/skills/worktree-discipline/` (first reference skill).
