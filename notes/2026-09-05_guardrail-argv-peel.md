# 2026-09-05 — Guardrail argv normalization (wrapper peeling, fail-closed)

Closes #483. The destruction (`Pāṇātipāta`) and privilege-escalation
(`Bhava-taṇhā`) guardrails resolved "the binary" as the first token that isn't a
`VAR=val` assignment. That stripped a leading assignment **and nothing else**, so
every destructive command hidden one layer down behind a transparent wrapper
walked straight past the check:

```
env rm -rf /   command rm -rf ~   timeout 5 rm -rf /   xargs rm -rf   sh -c 'rm -rf /'
```

Guardrails is the layer documented as *unoverridable* by permission config, so a
miss there is **silent under-blocking**, not a degraded prompt.

## What changed

- New `crates/bwoc-harness/src/policy/argv.rs`: `peel(segment) -> Peel` reduces a
  shell segment to the argv that will actually run — stripping the wrapper set
  (`env command builtin exec nohup stdbuf setsid nice ionice timeout`), unwrapping
  one `sh|bash|zsh|dash -c '<literal>'`, and recursing through nested wrappers up
  to a depth cap. Includes a minimal **quote-aware** tokenizer (the old
  `split_whitespace` broke `sh -c '…'` into five tokens).
- **Fail-closed** is the load-bearing property: an unmodelled wrapper flag
  (`env -C`, `env -S`, `timeout --foreground`), a stdin-fed runner (`xargs`), a
  non-literal `sh -c "$CMD"`, unbalanced quotes, or depth exhaustion all return
  `Peel::FailClosed`, which the callers turn into a block. *If you cannot see what
  the command invokes, do not certify it.*
- The **same** normalizer is wired into all three sites so they cannot drift:
  `guardrails::check_destruction`, `guardrails::check_privilege_escalation`, and
  `sandbox::scan_args` (the last peels per segment to also catch `env sudo …`).

## Decisions

- **Not a POSIX parser.** Took the load-bearing 20% (a wrapper table + one `sh -c`
  unwrap + fail-closed), explicitly not tree-sitter or a 2k-line lexer. No new
  deps.
- **`xargs` → always fail-closed.** Its operands come from stdin, invisible to a
  static argv view, so it can never be certified. Accepts a rare over-block of
  `find … | xargs rm <files>` as the price of never missing `xargs rm -rf`.
- **sandbox fail-closed segments are left to guardrails** (which runs ahead of
  execution and is authoritative); `scan_args` acts only on positively-resolved
  argv, keeping its defence-in-depth role from double-reporting.

## Tests

- `argv.rs`: the five bypass strings, leading assignments, nested wrappers,
  absolute-path wrappers, arg-taking flags (`nice -n`, `stdbuf -oL`), and the
  fail-closed cases (unmodelled flags, non-literal `-c`, unbalanced quotes, deep
  chains) — plus the tokenizer.
- `guardrails.rs`: end-to-end `check("run_command", …)` blocks all five bypasses
  and wrapped `sudo`, while a plain `timeout 60 cargo test` / `env FOO=bar cargo
  build` still passes (no over-block on ordinary wrapped builds).

## Related

- Issue #483; pairs with #484 (allow-rule substring), which reuses this peeler.
- Source `research/2026-08-23_grok-build-comparison.md`.
