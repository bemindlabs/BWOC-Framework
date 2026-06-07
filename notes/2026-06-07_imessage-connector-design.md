# 2026-06-07 — iMessage connector (design spec, Mac-only, free)

A fourth chat connector: `bwoc-connect imessage`, bridging Apple Messages to a
BWOC agent on the **same Mac**. **Spec only — not built.** Captures the design,
the two seams that don't fit iMessage as-is, and a decision the architect should
make before any code.

## Goal & hard constraints

- **Free** ($0): no API/subscription. Send via the public **AppleScript** surface
  (`osascript` → Messages.app); receive by reading the local **`chat.db`**.
- **Mac-only**: there is **no server/cross-platform iMessage API** (verified 2026
  — Apple offers none; every method drives Messages.app on a logged-in Mac). So
  this connector runs **only on a macOS agent host** signed into iMessage. The
  bemind deployment box is Ubuntu → it can **not** host this; target a Mac in the
  fleet (e.g. a Mac mini).
- **Reuse the bridge.** Like Discord, this should be a new `Transport` only —
  `run_bridge` / allow-list / mention-gate / group⇄team / session reuse unchanged.

## Free method (recommended MVP)

| Direction | Mechanism | Notes |
|-----------|-----------|-------|
| **send** | `osascript` → `tell application "Messages" to send <text> to …` | public AppleScript; **no SIP changes**. Needs **Automation** TCC grant. |
| **receive** | read `~/Library/Messages/chat.db` (SQLite, read-only) | needs **Full Disk Access** TCC grant. Poll `message WHERE ROWID >= offset AND is_from_me = 0` (offset = bridge cursor = last `update_id` + 1). |

No BlueBubbles, no `imessage-rs`, no private API for the MVP — just `osascript` +
a read-only SQLite poll. (BlueBubbles / `imessage-rs` become the upgrade path for
streaming/tapbacks/edit — see below.)

## Mapping onto the existing seams

- **`Transport::poll(offset)`** ↔ `chat.db` perfectly: the `message.ROWID` is a
  monotonic integer → use it as `Incoming.update_id`. The bridge advances
  `offset = update_id + 1` and the trait contract is "return `update_id >=
  offset`", so the query is `message … WHERE ROWID >= offset AND is_from_me = 0`
  (with `offset` starting at 0) — equivalently `ROWID > last_seen`, just stated to
  match the bridge's `>= offset` contract so no message is skipped. JOIN `handle`
  / `chat_message_join` / `chat`; `text` (or `attributedBody` on newer macOS —
  see risks). No long-poll, so the transport sleeps ~1–2s between reads (like the
  Discord queue's 1s tick).
- **`Transport::send`** ↔ `osascript` to the chat GUID / handle.
- **`is_group`** ← the `chat` row has >1 participant / `style`. **`mentions_bot`**
  ← iMessage group @mentions exist (stored in the message); gate on the Mac's own
  handle. Fits the mention model.

## The two things that DON'T fit (decision needed)

`Incoming.chat_id` / `from_user_id` and `TelegramConfig.allow_from` are **`i64`**.
iMessage identifies peers by **handle** (phone `+1…` / email / chat GUID) —
**strings**, not integers. Two options:

- **(A) Generalise ids to strings (recommended, principled).** Change the
  `Transport` seam to a `PeerId(String)` (Telegram/Discord store their numeric
  ids as decimal strings — lossless, and it retires the current Discord-snowflake-
  into-`i64` squeeze) and make `allow_from` a `Vec<String>`. Bigger diff (touches
  Telegram/Discord + tests) but it's the honest model and a one-time cost.
- **(B) Keep `i64`, map locally (lower churn, MVP).** Use `chat.db`'s integer
  `chat.ROWID` / `handle.ROWID` as the `i64` ids; add an iMessage-only
  `allow_handles: Vec<String>` to the config, resolved to ROWIDs at startup via
  `chat.db`; the transport keeps the reverse ROWID→handle map to address
  `osascript` sends. Confined to the iMessage transport, but `allow_handles`
  diverges from `allow_from` and ROWIDs aren't portable across Macs.

**Recommendation: (A)** if we expect more non-numeric platforms (Slack, Matrix,
iMessage all use string ids); **(B)** if iMessage is a one-off. Either way the
config gains a way to allow-list **handles**, since phone/email is what a human
actually knows.

## Streaming

The send-then-edit streaming (added for Telegram/Discord by the streaming PR
**#228**, which introduced `AgentSession::ask_streamed`, `Transport::edit`, and
`Transport::supports_edit`) **can't** work on the MVP free path: AppleScript
can't edit a sent message. So iMessage MVP is **non-streaming** — and the
architecture handles that gracefully: a transport that returns
`supports_edit() == false` (like LINE) makes the bridge send the reply once on
turn end. iMessage *does* have edit (iOS 16+); wiring it would
require the **BlueBubbles private API / `imessage-rs`** path (SIP disabled + dylib
into Messages) — an explicit upgrade, not MVP.

## Security / identity caveats (call these out to users)

- **No bot identity.** The agent speaks as the **Mac's own Apple ID** — replies
  look like they came from *you*. There's no separate bot account. This is the
  biggest UX/trust difference from Telegram/Discord.
- **Closed-by-default** allow-list (handles) carries over; non-allow-listed
  senders ignored. Harness session stays non-TTY → tool calls auto-deny (same
  fail-safe as the others). `chat.db` is opened **read-only**.
- **Apple ToS**: automating Messages / running a bot is against Apple's terms
  (personal-use automation only). Document it; don't ship it as a "bot platform".
- **TCC permissions**: Full Disk Access (chat.db) + Automation (Messages) — both
  are manual one-time grants; the connector must fail with a clear message if
  denied (not silently poll nothing).

## Daemon supervision

`bwoc-agent --serve` already spawns connectors from `connectors/<platform>.toml`
via the `KNOWN` table. Add `("imessage", "connectors/imessage.toml")` — but the
`bwoc-connect imessage` subcommand should hard-error early on non-macOS (and if
`osascript` / `chat.db` are absent), so a stray config on the Ubuntu host no-ops
loudly rather than crash-loops.

## Risks / open problems

- **`text` is increasingly `NULL`**, body moved to `attributedBody` (a typedstream
  blob) on recent macOS — the poll must decode it (or shell `osascript` to read
  the message). Verify on the target macOS version.
- **AppleScript send fragility**: Apple has broken/restricted send-to-arbitrary-
  number across releases; validate on the actual macOS version before relying on
  it. BlueBubbles is the fallback if AppleScript send is dead on that OS.
- **Group send addressing** by chat GUID via AppleScript is finicky; DM-first MVP
  is safer (groups as a follow-up).

## Recommendation

Build **DM-first, non-streaming, `osascript` send + `chat.db` poll**, option (A)
string ids if we're committing to more string-id platforms (else (B)). Gate the
whole thing behind `cfg(target_os = "macos")` + a runtime capability check. It's
genuinely free, but niche until an agent runs on a Mac — so **only build on an
explicit go**, not speculatively (Mattaññutā).

## Related

- `notes/2026-06-07_connect-subsystem-complete.md`, `notes/2026-06-07_connect-streaming.md` (added by streaming PR #228)
- `notes/2026-06-06_chat-connectors-design.md` (the seams this reuses)
- Refs: BlueBubbles (OSS Mac server + API), `jesec/imessage-rs` (Rust, BlueBubbles-compatible), `openclaw/imsg` (agent CLI)
