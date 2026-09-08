# 2026-09-07 — CI: retry the cross-backend ollama smoke run through a llama-server segfault

The **Cross-backend validation** workflow's `ollama` job failed once on a push to
`main` (run 34030942134, 2026-09-06) — 1 red in ~15 runs. Root cause from the
job log was **not** the port-bind line at teardown (a red herring) but:

```
transient error on qwen2.5:0.5b (attempt 1/3): HTTP 500:
  llama-server process has terminated: signal: segmentation fault (core dumped)
```

Ollama's model runner (`llama-server`) segfaulted on load. All three
`bwoc-harness` HTTP retries (the #486 429/5xx retry, working as intended) hit the
same crashed runner in one request cycle, so the harness gave up → the `ṭhiti`
step exited 1 → the job went red. Not a BWOC defect — an upstream ollama flake.

## What changed

`.github/workflows/cross-backend.yml` — the `ṭhiti` step now retries the whole
`bwoc run` up to **3×**, evicting the model (`ollama stop`) between attempts so a
fresh `llama-server` is spawned. It exits 0 on the first good run and fails only
if **every** attempt crashes — so a persistent segfault (a real regression) is
still caught, while a one-off flake is absorbed.

## Decisions

- **Retry, not version-chase.** The segfault is an ollama/llama-server bug; a
  step retry is robust regardless of ollama version, so `OLLAMA_VERSION` stays
  pinned for reproducibility.
- **Not `continue-on-error`.** That would blind the job to a genuine
  ollama-backend regression; a bounded retry keeps the signal.
- Parse the run's `.exit_code` / `.output` from the JSON envelope each attempt
  (as before), guarded with `//` defaults so a crash that emits no JSON still
  fails cleanly rather than erroring in `jq`.

## Status

Cross-backend is a push/schedule workflow, not a required PR gate — `main` was
already green (the next scheduled run passed). This removes the recurring
one-off red.

## Related

- Run 34030942134; workflow `.github/workflows/cross-backend.yml`.
