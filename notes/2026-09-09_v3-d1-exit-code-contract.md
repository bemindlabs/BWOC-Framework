# 2026-09-09 — v3 D1: one exit-code contract for the CLI

Closes gap **D1** from the v3 scope proposal (`notes/2026-09-08_v3-scope-proposal.md`,
Mac side). Stacked on `feat/v3-w1-schema-seam` (the compatibility-contract major)
as a PR **into that branch**, not main — folding the remaining breaking cleanup
into 3.0 so the fleet crosses one major boundary, not two.

## The problem

Every command family invented its own exit-code vocabulary. `bwoc check` returned
`1` for "violations found"; `bwoc workspace validate` returned `2` for the *same*
semantic — and `2` elsewhere means "usage / not found", so a script could not
tell a **missing** workspace from one **with violations**. The nine plugin fronts
each re-declared their own `0/1/2/4/255` block (drift waiting to happen). `council`
already used `3` for "not resolved" and `jira` `3` for "conflict" — a
negative-but-valid convention that existed in two places but nowhere else.

## What changed

- New `crates/bwoc-cli/src/exit.rs` — the single canonical table:
  `0` OK · `1` ERROR (command broke) · `2` USAGE (bad invocation / not found) ·
  **`3` FINDINGS** (ran fine, answer is negative) · `4` NO_PLUGIN ·
  `254` FAIL_COUNT_MAX (audit's count-encoding, the one exception) · `255` INTERNAL.
- **Behavioral remaps (breaking):** `check` violations `1 → 3` (both `run` and
  `run_all`), `workspace validate` violations `2 → 3`, `workspace prune` leftover
  orphans `2 → 3`. This extends the existing `council = 3` convention rather than
  inventing one. The load-bearing new distinction is **`1` vs `3`** — a crash vs.
  a negative finding — which CI could not tell apart before.
- The nine fronts (`accounting/audit/council/figma/gcloud/gws/jira/okr/resource`)
  keep their local const names but source the **values** from `crate::exit::*`, so
  they can never drift; `check.rs`/`workspace.rs` raw returns now reference the
  constants too.
- Documented in `docs/en/COMPATIBILITY.en.md` §Exit codes (+ TH parity) and
  CHANGELOG under the 3.0.0 §Changed.

## Decisions

- **`3` for findings, not "keep check=1".** In a major we fix it right: `1` means
  the tool failed, `3` means it ran and the result is "no". Worth the churn for
  the CI signal (fail-on-violations vs alert-on-crash).
- **Re-point, don't rename.** Fronts keep `EXIT_*` local names (zero call-site
  churn, lower risk); only the definitions point at the canonical module. The
  risk stays concentrated in the two behavioral remaps.
- **audit's count-encoding kept** as a documented exception — folding it to plain
  `3` would delete a feature.

## Verification

- `cargo build -p bwoc-cli` ✓ · `cargo clippy -p bwoc-cli` clean · fmt applied.
- `cargo test -p bwoc-cli --bins check` 187 pass · `… workspace` 26 pass · no test
  asserted the old `check=1` / `validate=2` values.
- Full `cargo test --workspace` deferred to CI — the Mac disk hit 100% mid-session
  (see status), so heavy local runs were constrained; the branch built + unit
  tests passed before that.

## Related

- v3 scope proposal (D1); branch `feat/v3-w1-schema-seam`; source
  `crates/bwoc-cli/src/exit.rs`.
