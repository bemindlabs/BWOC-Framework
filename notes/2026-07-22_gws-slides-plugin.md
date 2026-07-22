# 2026-07-22 — gws-slides: Google Slides read + in-place write

Final slice of the gws editor-app series (Docs → Sheets → Slides). Adds `gws-slides`: reads a presentation (`presentations.get`) and edits it via `presentations.batchUpdate` (the general write verb) plus the convenience `replace-all-text`. Reuses the write-verb operator-confirm gate and the `resolve_docs_requests` helper introduced with `gws-docs`.

## What changed

- **New plugin** `modules/plugins/gws/gws-slides/`: `manifest.toml`, `gws.sh` (verbs `get` / `batch-update` / `replace-all-text`), `SPEC.md` + `SPEC.th.md`. Sources `gws-auth`; scope `https://www.googleapis.com/auth/presentations`.
- **`gws.rs`**: `slides` command tree + request builders + handlers + render. Read via `run_read_verb`; writes via the shared `run_write_verb` gate; batch-update reuses `resolve_docs_requests` (identical requests-array validation). Module-doc verb table + writes section updated.
- **`check.rs`**: `GwsService::Slides` + Google Presentation resource shape (`presentation_id` / `title` required, `slide_count` / `web_view_link` optional).
- **Docs (EN+TH parity)**: `docs/{en,th}/PLUGINS`: Google Presentation resource shape + kind description now lists all three write-capable services (docs/sheets/slides).
- **Tests**: 2 new (verb parsing incl the requests ArgGroup + find-required, request builders). `cargo test -p bwoc-cli` = 807. jq projections smoke-tested offline.

## Decisions

- **Mirror `gws-docs`** exactly (get + batch-update + replace-all-text) rather than invent per-shape verbs — `batchUpdate` is the honest, complete Slides write surface; the requests-array validation is literally the same code (`resolve_docs_requests`).
- **`get` projects `slide_ids`** (objectIds) so a follow-up `batch-update` can target specific slides without a second round-trip; full slide bodies are intentionally not pulled (Mattaññutā / Adinnādāna — minimal surface).

## Status / deferred

- Shipped `gws-slides` — **completes the Docs/Sheets/Slides series**. The gws kind now has three write-capable services, all behind the one operator-confirm gate.
- Deferred across the series: Drive/Gmail/Calendar writes (send/insert/upload); Sheets `spreadsheets.batchUpdate` (structural); higher-level per-shape verbs. Secondary #354 (resolvable plugin install-source registry) still deferred.

## Related (links)

- Series: #354 / `gws-docs` (`2026-07-22_gws-docs-plugin.md`), `gws-sheets` (`2026-07-22_gws-sheets-plugin.md`).
- `modules/plugins/gws/gws-slides/`; `gws.rs` (`run_slides_*`), `check.rs` (`GwsService::Slides`).
