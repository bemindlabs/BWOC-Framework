#!/usr/bin/env bash
# inbox-auto-reply.sh — Claude Code Stop hook that mirrors this agent's last
# assistant response back onto the BWOC inbox bus when the triggering user
# prompt carried a `[bwoc inbox msg-XYZ from <sender>]` marker.
#
# The marker is written by `bwoc send` (see notify_tmux in
# crates/bwoc-cli/src/send.rs) when it wakes a recipient's tmux session/pane.
# When the agent finishes that turn, this hook reads the *marker* (sender +
# msg-id) from the transcript's last user event, takes the reply text from the
# Stop payload's `last_assistant_message` (the transcript may not have flushed
# the just-finished turn yet — a race that silently dropped replies; it falls
# back to scanning the transcript when the field is absent), and shells out to:
#
#   bwoc send --from <self> --reply-to <msg-id> --no-wakeup <sender> "<reply>"
#
# Backend neutrality: this hook is Claude-specific (Stop event). The
# equivalent surface on other backends should follow the same protocol —
# parse the inbox marker, post a reply with --reply-to. See
# interconnect/messaging.md §Wakeup & Auto-Reply.
#
# Stdin: Claude Code Stop event JSON
#   {"session_id":..., "transcript_path":..., "cwd":...,
#    "stop_hook_active": false, ...}
#
# This hook is a no-op (exit 0, silent) when:
#   - stop_hook_active is true                       (loop guard)
#   - transcript_path is missing or unreadable       (nothing to mirror)
#   - this agent's config.manifest.json is missing   (no codename to send as)
#   - the most recent user message lacks the marker  (not a bus turn)
#   - the most recent assistant message is empty
#   - the original sender is "user"                  (humans don't need bus replies)

set -euo pipefail

# Cache stdin payload — python re-reads it below.
payload=$(cat)

python3 - "$payload" <<'PY' || true
import json, os, re, subprocess, sys

payload_raw = sys.argv[1]

try:
    payload = json.loads(payload_raw) if payload_raw.strip() else {}
except json.JSONDecodeError:
    sys.exit(0)

# Loop guard.
if payload.get("stop_hook_active"):
    sys.exit(0)

transcript_path = payload.get("transcript_path") or ""
cwd = payload.get("cwd") or os.getcwd()

if not transcript_path or not os.path.isfile(transcript_path):
    sys.exit(0)

# Resolve this agent's codename by walking up from cwd looking for
# config.manifest.json (agent root) — same chain bwoc itself uses.
def find_manifest(start):
    cur = os.path.abspath(start)
    while True:
        cand = os.path.join(cur, "config.manifest.json")
        if os.path.isfile(cand):
            return cand
        parent = os.path.dirname(cur)
        if parent == cur:
            return None
        cur = parent

manifest_path = find_manifest(cwd)
if not manifest_path:
    sys.exit(0)

try:
    with open(manifest_path, encoding="utf-8") as f:
        manifest = json.load(f)
except Exception:
    sys.exit(0)

self_id = manifest.get("agentId") or ""
if not self_id:
    sys.exit(0)

# bwoc send accepts bare or canonical form for --from. We pass canonical.

def extract_text(content):
    """Claude transcript content can be a string or a list of blocks."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for block in content:
            if isinstance(block, dict) and block.get("type") == "text":
                parts.append(block.get("text", ""))
        return "".join(parts)
    return ""

events = []
try:
    with open(transcript_path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
except Exception:
    sys.exit(0)

# Most recent user event.
last_user_idx = None
for i in range(len(events) - 1, -1, -1):
    if events[i].get("type") == "user":
        last_user_idx = i
        break

if last_user_idx is None:
    sys.exit(0)

user_msg = events[last_user_idx].get("message") or {}
user_text = extract_text(user_msg.get("content", ""))

# Marker written by bwoc send notify_tmux:
#   [bwoc inbox msg-XXXX from <sender>] <message>
marker = re.search(
    r"\[bwoc inbox (msg-[0-9A-Za-z._-]+) from ([a-z0-9][a-z0-9-]*)",
    user_text,
)
if not marker:
    sys.exit(0)

reply_to_msg_id = marker.group(1)
sender = marker.group(2)

# Don't bus-reply to the human operator — they read logs/inbox directly.
if sender == "user":
    sys.exit(0)

# The assistant's reply text. Prefer the Stop payload's `last_assistant_message`
# — at Stop time the transcript file may not have flushed the just-finished
# assistant turn yet, so re-parsing it from disk races and silently yields no
# reply (the bug that made bus auto-reply never fire). Fall back to scanning the
# transcript for older Claude Code versions that don't provide the field.
# Preserve leading whitespace (an indented Markdown code block at the start of
# the reply is significant); only .strip() to *test* for emptiness.
reply_text = payload.get("last_assistant_message") or ""
if not reply_text.strip():
    for i in range(last_user_idx + 1, len(events)):
        if events[i].get("type") == "assistant":
            text = extract_text(events[i].get("message", {}).get("content", ""))
            if text.strip():
                reply_text = text  # keep updating to get the latest

if not reply_text.strip():
    sys.exit(0)

# Trim trailing whitespace/newlines only — leading is kept (see above).
reply_text = reply_text.rstrip()

# Cap so an over-long assistant turn doesn't bloat the recipient's inbox.
MAX_LEN = 4000
reply = reply_text
if len(reply) > MAX_LEN:
    reply = reply[:MAX_LEN] + "…[truncated]"

# Fire-and-forget. --no-wakeup so we don't ping the sender's TUI for a reply
# the sender is presumably actively reading anyway (their daemon poll will
# surface it); --from carries this agent's identity so the recipient's trust
# gate evaluates against our manifest.
result = subprocess.run(
    [
        "bwoc", "send",
        "--from", self_id,
        "--reply-to", reply_to_msg_id,
        "--no-wakeup",
        sender,
        reply,
    ],
    check=False,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)

if result.returncode != 0:
    import datetime
    ts = datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ")
    stderr_snippet = (result.stderr or b"").decode("utf-8", errors="replace")[:500]
    diag = f"{ts} inbox-auto-reply: send to {sender} failed (exit {result.returncode}): {stderr_snippet}\n"
    log_path = os.path.join(os.path.dirname(manifest_path), ".bwoc", "agent.log")
    try:
        os.makedirs(os.path.dirname(log_path), exist_ok=True)
        with open(log_path, "a", encoding="utf-8") as lf:
            lf.write(diag)
    except OSError:
        pass  # log write failed — still exit 0; hook must not block the agent
PY
