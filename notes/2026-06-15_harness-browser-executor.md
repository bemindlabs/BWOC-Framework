# 2026-06-15 — Computer-use P1: headless-browser executor

Second computer-use slice (after the P0 spike). Turns the `ComputerExecutor`
seam into a **real** executor backed by a headless Chromium over CDP — the
lighter of the two P1 paths (chosen by the owner over a full Xvfb desktop
container): no display server, runs on macOS/Linux/Windows, covers the bulk of
web-automation use-cases.

## What changed

New `crates/bwoc-harness/src/tools/browser.rs` (module registered in
`tools/mod.rs`):

- **Pure layer (always compiled + tested, no browser dep):** `cdp_plan(action)`
  maps each `ComputerAction` to the ordered CDP calls that realize it
  (`Input.dispatchMouseEvent` press/release for a click, `mouseWheel` deltas for
  scroll, `Input.insertText`, `Input.dispatchKeyEvent` down/up, etc.), plus
  `scroll_deltas` / `mouse_button_name` helpers. 8 unit tests.
- **Feature-gated `BrowserExecutor` (the `browser` feature):** launches headless
  Chromium via `chromiumoxide`, drives one `Page`, implements `ComputerExecutor`
  (screenshot → PNG bytes; mouse/keyboard/scroll via typed CDP params; `Wait` →
  sleep). A `#[cfg(feature="browser")] #[ignore]` live smoke test launches real
  Chrome, screenshots a `data:` page, and dispatches input.

`Cargo.toml`: `chromiumoxide` added **optional**, behind a new `browser` feature
(mirrors the existing `otel` pattern). Default build pulls **zero** browser deps.

## Decisions

- **Headless browser, not Xvfb container.** Owner's call: far lower weight,
  cross-platform, no display server; the full GUI-desktop path stays deferred for
  when a non-web target actually appears (Mattaññutā).
- **Pure mapping always compiled; live executor gated.** The CDP mapping is the
  reviewable, testable core and exists in every build; only the heavy
  chromiumoxide + runtime-Chrome dependency is gated. Keeps the default CI matrix
  green and light (the live test is `#[ignore]` — no browser in CI).
- **Verified for real.** `cargo build --features browser` compiles clean and the
  live smoke test passes against installed Chrome locally — the chromiumoxide
  wiring is not feature-gated bitrot, it actually drives a browser.

## Status / deferred

- **Not wired into the live agent loop / registry yet** — same as the spike, the
  executor is a building block. Loop wiring + provider native-tool passthrough is
  still ahead.
- **P2 (security) unchanged and still required before any real use:** screenshots
  + page content are untrusted input → taint propagation; gate `computer`/browser
  behind a high capability tier; `ask` every action by default; autoprocess must
  refuse it unless explicitly granted (mirror t30).
- **CI coverage of the `browser` feature** is not added here (no browser in the
  matrix); if desired later, a dedicated job with Chrome installed can run the
  `--ignored` smoke test. Surfaced, not silently skipped.
- Key-chord parsing is minimal (single key string passed through); richer chords
  (`ctrl+s` → modifiers) are a later refinement.

## Related

- `crates/bwoc-harness/src/tools/browser.rs`
- `notes/2026-06-15_harness-computer-use-spike.md` (P0 — the seam this builds on)
