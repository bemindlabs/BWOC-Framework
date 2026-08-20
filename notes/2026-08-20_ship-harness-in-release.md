# 2026-08-20 — Ship `bwoc-harness` in release artifacts (#460)

`bwoc-harness` was shipped by **no install path at all** — not the release
archives, not the Homebrew formula, not `scripts/install.sh`, and not crates.io.
Every non-contributor install therefore had a `bwoc` that could not spawn the
component running the agentic loop. Reported against `v2026.8.20-0` — the
release cut hours earlier — and verified end-to-end before fixing.

## Verified first (all claims true)

| Claim | Verdict |
|---|---|
| Release tarball has only `bwoc` + `bwoc-agent` | **true** — `tar tzf` on the published `aarch64-apple-darwin` archive |
| `Formula/bwoc.rb` installs only those two | **true** — `install` block, lines 52–53 |
| `cargo install bwoc-harness` 404s | **true** — *and* `bwoc` itself 404s: **nothing** is published to crates.io, so the whole hint class is wrong, not just this crate |
| The lookup fails at spawn | **true** — `sibling_binary` tries exe-dir → `CARGO_BIN_EXE` → `PATH`; the harness is in none |

**Root cause:** `cargo build --release --workspace` *does* build `bwoc-harness`
in CI — the packaging step simply never copied it. A one-line class of bug that
survived many releases because nothing asserted the archive's contents.

Also found while verifying (not in the report): **`scripts/install.sh` has the
same gap** — it installs `bwoc-cli` and `bwoc-agent` only, so even the
documented from-source path left users without the harness.

## What changed

- **`.github/workflows/release.yml`** — both packaging steps (tar.gz + zip) now
  copy `bwoc-harness` / `bwoc-harness.exe`, and each **asserts all three binaries
  are present**, failing the build rather than publishing a silently-incomplete
  archive. The missing assertion is why this shipped unnoticed.
- **`scripts/install.sh`** — now `[3/3]`, installing `bwoc-harness` too; `--check`
  and `--uninstall` and the header docs cover all three.
- **`crates/bwoc-cli/src/doctor.rs`** — dropped the false `cargo install
  bwoc-harness` hint (no BWOC crate is on crates.io) in favour of
  `scripts/install.sh`.
- **Docs** — `README.md` install section and `RELEASING.en/th.md` (EN/TH parity)
  now state the three-binary set and the new packaging assertion.

## Deliberately NOT in this PR — the formula sequencing trap

`Formula/bwoc.rb` still installs two binaries, **on purpose**. `bump-formula.sh`
rewrites only `version` / `url` / `sha256`; the `install` block is static. So
adding `bin.install "bwoc-harness"` while the formula still points at
`v2026.8.20-0` — whose archives lack the harness — would break `brew install`
**immediately** for every user.

Correct order (no broken window):

1. this PR — pipeline + source install fixed;
2. cut a patch release, whose archives now contain the harness;
3. the formula-bump PR adds `bin.install "bwoc-harness"` **together with** the
   new urls/shas, atomically.

## Verification

- `release.yml` parses as valid YAML; `bash -n scripts/install.sh` clean;
  `./scripts/install.sh --check` reports all three binaries.
- Packaging simulated locally against the real binaries: the archive contains
  `bwoc`, `bwoc-agent`, `bwoc-harness`, and removing one makes the guard fire.
- `cargo test -p bwoc-cli --bin bwoc doctor` — 10 pass.

## Related

- Issue #460 (reporter: kla-ondemand). Release that exposed it: `v2026.8.20-0`.
- Resolver: `bwoc-core::exec::sibling_binary` (exe-dir → `CARGO_BIN_EXE` → `PATH`).
