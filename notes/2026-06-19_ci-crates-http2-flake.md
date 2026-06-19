# 2026-06-19 — Fix the recurring crates.io HTTP/2 CI flake

Across the warm-mode PRs (#315/#316/#317) the macOS `build + test` job failed
repeatedly within ~8–12s with:

```
error: failed to get `itoa` as a dependency of package `serde_json`
  download of it/oa/itoa failed
  curl failed … [16] Error in the HTTP2 framing layer
```

Not a code problem — a transient in cargo's **HTTP/2 multiplexed** fetch from
crates.io. Every occurrence cleared on a plain re-run, but it was burning
re-runs on nearly every PR.

## What changed

Added two cargo network env vars to the workflow-level `env:` of every Rust
workflow (`ci.yml`, `cross-backend.yml`, `release.yml` — `docs.yml` builds no
Rust):

- `CARGO_HTTP_MULTIPLEXING: false` — force HTTP/1.1 for the registry download.
  HTTP/2 multiplexing is the exact layer the error names; disabling it removes
  the failure mode (at a negligible cost for the small number of fetches a CI
  job makes).
- `CARGO_NET_RETRY: 10` — retry any remaining transient network error instead of
  failing the job on the first hiccup.

## Decisions

- **Workflow-level `env:`, not per-step.** The fetch happens in several steps
  (clippy, build, test); one env block covers them all.
- **All three Rust workflows, not just `ci.yml`.** The flake is a registry-fetch
  property, not a ci-specific one — release and cross-backend `cargo build` from
  the same registry and would flake identically. Same two lines, same root cause.

## Status / deferred

- If crates.io HTTP/2 stabilizes, these are harmless to keep (HTTP/1.1 + retries
  are strictly more robust for CI). No revert planned.

## Related

- `.github/workflows/{ci,cross-backend,release}.yml`; surfaced during #301.
