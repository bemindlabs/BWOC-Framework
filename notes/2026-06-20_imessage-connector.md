# 2026-06-20 — iMessage connector (built — Mac-only MVP, #229)

Builds the fourth chat connector, `bwoc-connect imessage`, from the 2026-06-07
design spec (`notes/2026-06-07_imessage-connector-design.md`). DM-first,
non-streaming, free, **macOS-only**. The architect chose **option B** (keep the
`i64` seam + a string allow-list) over option A (generalise all ids to strings).

## What changed

- **`crates/bwoc-connect/src/imessage.rs`** — a new `Transport`:
  - **receive**: poll `~/Library/Messages/chat.db` (SQLite, **read-only**) for
    `message.ROWID >= offset AND is_from_me = 0`, joined to `handle` for the
    sender address. `ROWID` is the bridge's monotonic poll offset.
  - **send**: `osascript` → Messages.app (`send … to participant …`).
  - `supports_edit() == false` → the bridge single-sends on turn end (like LINE).
- **Option B identity**: handles (phone/email) are hashed to a stable `i64` via
  `imessage::hash_id` (same scheme as `line::hash_id`), so they ride the existing
  `Incoming`/allow-list seam unchanged. The transport keeps a `hash → handle` map
  built during `poll` so `send` can address the `osascript` back to the real
  handle. Config gains `[imessage] allow_handles = [...]`, folded into
  `allow_from` in `main` (exactly the LINE pattern).
- **macOS-gating**: the live transport is in a `#[cfg(target_os = "macos")]`
  submodule; `main` builds it on macOS and **hard-errors** elsewhere. The
  daemon's `KNOWN` connector table gains `imessage`, so a stray config on the
  Ubuntu host no-ops loudly rather than crash-loops.
- **No token**: `token_env` became `Option` — iMessage drives the local app, so
  it resolves no credential (it needs Full Disk Access + Automation TCC grants
  instead, checked at startup with a clear error).
- **Docs**: `docs/{en,th}/CONNECTORS.md` updated "three platforms" → "four"
  (table row, config block, no-token note, security posture, limitations), EN/TH
  in lock-step.

## Decisions

- **Option B (i64 + string allow-list), per the architect.** Confined to the
  iMessage transport; no churn to Telegram/Discord/LINE. Uses `hash_id` rather
  than the spec's `chat.db` ROWIDs — ROWIDs aren't portable across Macs, the hash
  is, and it matches LINE's existing precedent exactly.
- **`attributedBody` best-effort decode.** Recent macOS often stores the body in
  the `attributedBody` typedstream blob with `text` NULL. `decode_message_text`
  prefers `text`, else extracts the length-prefixed string after the `NSString`
  marker, returning `None` (skip the row) on any inconsistency — never surface
  garbage. Verify against the live macOS version before relying on it.
- **Testable core vs live edge.** The pure helpers (`hash_id`,
  `escape_applescript`, `build_send_script`, `decode_message_text`) +
  the SQLite `read_new` mapping (against a temp `chat.db`) are unit-tested; the
  `osascript` send and a real Messages DB are the macOS-only, eyeball-reviewed
  edge (no iMessage in CI). Mirrors the LINE connector's test depth.

## Security / caveats (surfaced in docs)

- **No bot identity** — the agent speaks as the Mac's own Apple ID; replies look
  like they came from you. **Apple ToS**: automating Messages is personal-use
  only — a personal bridge, not a public bot platform. `chat.db` is **read-only**.
- Closed-by-default allow-list carries over; the bridged session is non-TTY so
  `ask`-mode tools fail-closed (a remote sender can't approve a tool).

## Status / deferred

- Built: DM-first, non-streaming MVP. **Deferred**: group rooms (chat-GUID
  addressing), edit/streaming (BlueBubbles / `imessage-rs` private API), media.
- Live end-to-end (real iMessage send/receive) needs a Mac signed into iMessage
  with the two TCC grants — manual verification, not CI.

## Related

- issue/PR #229 (the design spec); `crates/bwoc-connect/src/{imessage,lib,main}.rs`,
  `crates/bwoc-agent/src/connectors.rs`, `docs/{en,th}/CONNECTORS.md`.
