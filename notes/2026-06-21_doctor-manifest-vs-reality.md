# 2026-06-21 — `bwoc doctor`: manifest-vs-reality checks (#323)

`bwoc doctor` already audited environment + workspace hygiene (symlinks, stale
pids/sockets, cursors, oversize logs). #323 asked for the missing **static
manifest-vs-reality integrity** layer — drift that goes undetected until an agent
fails at runtime. Extended the existing command rather than adding a new one
(the PASS/WARN/FAIL/FIXED + `--auto` + `--json` scaffold was already exactly
right).

## What changed (3 new per-agent checks)

- **`manifest: <agent>`** — `config.manifest.json` present, valid JSON, and
  **fully substituted** (no `{{placeholder}}` tokens left from the template).
  An unincarnated/half-incarnated agent FAILs. Read-only (the correct values are
  the operator's to choose).
- **`agent key: <agent>`** — when `.bwoc/agent.key` exists, its Unix perms must
  be `0600`; a group/other-readable private key FAILs, and `--auto` chmods it.
  Absent key = skipped (not every agent is trust/signing-enabled). Unix-only
  (no mode bits on Windows).
- **`models: <agent>`** — the headline. For each agent whose `backend == "ollama"`,
  verify `primaryModel`/`fallbackModel` are actually installed (queries Ollama
  `/api/tags`). A referenced-but-missing model FAILs — the exact pre-`ollama rm`
  safety check #323 wanted. Ollama unreachable → one WARN (skipped), non-ollama
  backends → skipped (vendor model ids we can't verify).

## Decisions

- **Extend, don't add a `doctor`-twin.** All of #323's listed checks are static
  integrity audits; they belong with the existing sweeps under one exit code and
  one `--json` shape. No new command surface (Mattaññutā).
- **Raw HTTP for `/api/tags`, no HTTP crate.** doctor deliberately uses only
  `std::net` (the existing ollama endpoint probe). The model check does a minimal
  HTTP/1.1 `GET` over `TcpStream` and parses `models[].name` — keeps `bwoc-cli`
  HTTP-dep-free. Any connect/read/parse failure degrades to WARN, never a crash.
- **Tag-lenient model match** — a manifest `gemma4` matches an installed
  `gemma4:latest` (and vice-versa); exact `gemma4:9b` matches itself. Avoids
  false "not installed" on the common bare-name-vs-`:latest` mismatch.
- **Key: check-if-present, skip-if-absent.** A world-readable private key is a
  real exposure (FAIL/fix); a missing key is not a failure (many agents never
  sign). Avoids false positives on the majority.

## Verification

9 doctor unit tests (incl. placeholder detection + dedup, tag-lenient
`model_present`, a manifest FAIL on an unsubstituted `{{agentId}}`). Live smoke
against the real 20-agent workspace: all manifests + keys PASS; the only FAILs
are pre-existing stale pid/socket artifacts (the existing checks, working). fmt +
clippy clean.

## Status / deferred

- The reverse agents.toml↔dir check (a dir on disk **not** in the registry) is
  not added — an untracked dir isn't broken, just untracked (low value). The
  forward direction (registry → missing dir) is already a FAIL.

## Related

- issue #323; `crates/bwoc-cli/src/doctor.rs`; `bwoc_core::manifest::Manifest`.
