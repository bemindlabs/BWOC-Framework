# 2026-08-06 — inbox auto-reply: use the Stop payload's last_assistant_message

The bus auto-reply Stop hook (`modules/agent-template/.claude/hooks/inbox-auto-reply.sh`)
silently never replied in practice: an agent woken by a bus message *did*
respond, but the recipient's reply never made it back to the sender's inbox.

## Root cause (found by live e2e)

The hook re-parsed the **transcript file** for the assistant's reply text. But at
`Stop` time Claude Code has not necessarily flushed the just-finished assistant
turn to the transcript `.jsonl` yet — so the scan found no assistant text after
the marked user prompt and hit the `sys.exit(0)` no-op. Running the hook
*manually* worked (the transcript had settled by then), which is exactly why the
original mechanism — shipped without a live two-process test — looked fine.

The Stop payload Claude passes already contains the reply verbatim:
`{"hook_event_name":"Stop","last_assistant_message":"…","transcript_path":…}`.

## Fix

Prefer `payload["last_assistant_message"]`; fall back to the transcript scan only
when the field is absent (older Claude Code). The marker (sender + `msg-id`) is
still read from the transcript's last **user** event, which *is* reliably flushed
(it was typed before the turn).

## Verified — 3 agents, two-way, real Claude CLI backend (bemind)

Three `--backend claude` agents in tmux; `bwoc send` between each pair:

```
agent-aaa ← agent-bbb  (replyTo ✓): "Hi agent-aaa, hope you're doing well!"
agent-bbb ← agent-ccc  (replyTo ✓): "Hey agent-bbb, nice to see you again!"
agent-ccc ← agent-aaa  (replyTo ✓): "Hi agent-ccc — good to see you!"
```

Each round-trip = send→**wake** (the #411 `notify_tmux` session-resolution fix)
→ recipient responds → **Stop hook posts the threaded reply**. Both halves now
verified end-to-end.

## Related

- [[2026-05-23_inbox-wakeup-and-auto-reply]] — the original (unverified) hook.
- [[2026-08-06_inbox-wakeup-session-resolution]] — the wake half (#411).
