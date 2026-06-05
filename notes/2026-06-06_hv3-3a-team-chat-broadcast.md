# 2026-06-06 — HV3-3a: Team chat broadcast

Third piece of HV3-3 (Saṅgha collaboration; plan:
`notes/2026-06-05_harness-v3-plan.md`). Agents in a team can now see each
other's chat replies. With HV3-3b (worker result envelope) shipped, the only
remaining HV3-3 item is **(c) the peer-review gate**, still gated on the
architect's reviewer-selection decision.

## What changed

- **`bwoc-core::team`** — `TeamChatMessage { from, text, ts }` +
  `parse_chat`/`render_chat` (+ `to_line`). One JSON object per line in
  `.bwoc/teams/<team-id>/chat.jsonl`: the same append-only-JSONL storage model
  as `tasks.jsonl`, so the shared filesystem *is* the broadcast medium — no
  pub/sub, no broker. The race-safe-append lock stays the host's concern (as
  with tasks); core owns only the data model.
- **`bwoc-harness::chat_session`** — `ChatConfig.team_chat_log: Option<PathBuf>`
  opts a session in. `drive()` keeps a `team_seen` cursor; before each user
  turn `inject_peer_messages` folds teammate lines posted since the last turn
  into one "Team conversation (Saṅgha)" system note (skipping this agent's own
  lines), and after the turn `append_team_message` appends the reply
  (append-mode write). `last_assistant_text` picks the reply to broadcast.
- **`bwoc-harness` CLI** — `--chat --team-chat <path>` carries the resolved log
  path into `ChatConfig`.

## Decisions

- **File-based shared log, not live pub/sub.** BWOC sessions are
  human-paced and filesystem-coordinated already (tasks.jsonl + lock). A shared
  append-only log gives broadcast for free, survives restarts, and is directly
  auditable; a broker would add operational state for no current gain. (The
  scoping survey reached the same conclusion.)
- **Session-level opt-in (`--team-chat`), not a per-message broadcast verb.**
  Joining the channel is the opt-in; every reply in that session is shared.
  This needs **no chat-protocol change** — peer context arrives as a system
  note the agent reads, so the existing TUI frontend is untouched. A per-message
  `ChatInput::Broadcast` / `ChatEvent::TeamMessage` pair (so a frontend can
  render peer messages distinctly and the human can choose what to broadcast) is
  a deliberate **slice 2**, deferred until a frontend needs it (Mattaññutā — no
  protocol surface without a consumer).
- **Cursor resync, self-exclusion.** `team_seen` advances to the log length on
  every read (so a truncated/rewritten log resyncs rather than mis-injects), and
  `from != agent` filtering means an agent never sees its own replies echoed.
- **Harness flag takes a resolved path** (mirrors `--lead --tasks <path>`):
  workspace→team→`chat.jsonl` resolution belongs to the host
  (`bwoc chat --team`), keeping the harness path-agnostic. That CLI wiring in
  `bwoc-cli` is the natural next slice.

## Tests

- `team.rs`: `to_line` single-line roundtrip, `parse_chat` skips blanks /
  preserves order / rejects malformed, `render_chat`↔`parse_chat` roundtrip.
- `chat_session.rs`: inject skips self + advances cursor + no-op on no-new;
  append-then-peer-sees-it-but-sender-does-not (incl. parent-dir creation);
  missing log is a no-op; `last_assistant_text` picks the final non-empty reply.

## Status / deferred

- HV3-3a (engine slice) done. **Slice (1) — `bwoc chat --team` CLI wiring —
  now also done** (separate PR): `ChatArgs.team` → membership-checked
  `crate::sangha::load_team` + `team_chat_jsonl_path` → `bwoc_tui::TuiArgs.
  team_chat` → harness `--team-chat`. Only harness-backed `--tui` sessions can
  use it (vendor CLIs speak their own protocol); `--team` elsewhere warns and
  runs solo. `bwoc-tui::harness_argv` gained the trailing `--team-chat` pair.
  **Still deferred:** (2) chat-protocol `Broadcast` / `TeamMessage` variants +
  TUI rendering of peer messages as distinct events (today peer context reaches
  the agent's context but the human sees it only via the agent's replies).
- Remaining in HV3-3: **(c) peer-review gate** — needs the reviewer-selection
  decision (fixed / round-robin / manifest-declared). HV3-5 MCP-vs-A2A still open.

## Related

- `crates/bwoc-core/src/team.rs`, `crates/bwoc-harness/src/chat_session.rs`,
  `crates/bwoc-harness/src/main.rs`
- `notes/2026-06-05_hv3-3b-worker-result-envelope.md` (sibling HV3-3 piece)
