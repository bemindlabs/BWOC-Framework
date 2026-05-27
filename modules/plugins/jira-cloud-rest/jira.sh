#!/usr/bin/env bash
#
# jira-cloud-rest — Atlassian Jira Cloud REST v3 integration adapter (BWOC-43).
#
# The reference implementation of the `jira` plugin kind. Dispatched by the
# `bwoc jira` CLI (BWOC-42), which owns argument parsing, the sync ledger I/O,
# the auth gate, and the write-confirmation gate. This entry owns the HTTP:
# project-scoped JQL reads and gated status transitions against Atlassian
# Cloud REST v3. Contract: docs/en/PLUGINS.en.md §"Jira Issue Mapping Schema"
# + notes/2026-05-27_jira-plugin-architecture.md.
#
# ── Invocation contract ────────────────────────────────────────────────────
# The CLI spawns this script with:
#   stdin                  one-line JSON request, e.g.
#                          {"operation":"query","jql":"...","start_at":0,"max_results":50}
#   BWOC_JIRA_OPERATION    the operation name (query | transition | sync)
#   BWOC_WORKSPACE         absolute workspace root (sync reads .scrum/jira-sync.json)
#   BWOC_PLUGIN_DIR        absolute path to this plugin's directory
#   BWOC_JIRA_EMAIL        Basic-auth username half          (inherited env)
#   BWOC_JIRA_TOKEN        Basic-auth password half — SECRET (inherited env)
#   BWOC_JIRA_BASE_URL     https://<site>.atlassian.net      (inherited env)
#   BWOC_JIRA_PROJECT      (optional) project key for JQL scoping
#
# On success: exit 0 and a single JSON object on stdout (the CLI parses it).
# On error:   a human message on stderr + non-zero exit (the CLI surfaces it).
#
# ── Security (Sila — Adinnādāna) ───────────────────────────────────────────
# The token is read from the environment and handed to curl's -u only. It is
# never echoed, never written to a file, and never placed in any JSON output.
# auth.toml in this directory ships the auth SHAPE with EMPTY placeholders only.

set -euo pipefail

PLUGIN="jira-cloud-rest"
API_BASE="/rest/api/3"
MAX_ATTEMPTS=4          # 429 backoff: total tries before giving up
MAX_PAGE=100            # Atlassian maxResults ceiling (bounded reads — Mattaññutā)

# Globals set by _curl_retry.
HTTP_STATUS=0
HTTP_BODY=""

# ── stdin + dependencies ───────────────────────────────────────────────────

REQUEST="$(cat || true)"

for cmd in jq curl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN: required command '$cmd' not found on PATH" >&2
    exit 1
  fi
done

emit_error_json() { # code, message  — structured twin of the stderr diagnostic
  jq -n --arg code "$1" --arg msg "$2" \
    '{ok:false, plugin:"jira-cloud-rest", error:$code, message:$msg}'
}

# Operation: prefer the stdin request, fall back to the env var.
OPERATION=""
if [[ -n "$REQUEST" ]]; then
  OPERATION="$(printf '%s' "$REQUEST" | jq -r '.operation // empty' 2>/dev/null || true)"
fi
if [[ -z "$OPERATION" ]]; then OPERATION="${BWOC_JIRA_OPERATION:-}"; fi
if [[ -z "$OPERATION" ]]; then
  printf '%s\n' "$PLUGIN: no operation (set BWOC_JIRA_OPERATION or pipe a JSON request carrying .operation)" >&2
  exit 2
fi

# ── auth (env only; never logged) ──────────────────────────────────────────

EMAIL="${BWOC_JIRA_EMAIL:-}"
TOKEN="${BWOC_JIRA_TOKEN:-}"
BASE_URL="${BWOC_JIRA_BASE_URL:-}"

require_auth() {
  local missing=()
  [[ -n "$EMAIL"    ]] || missing+=("BWOC_JIRA_EMAIL")
  [[ -n "$TOKEN"    ]] || missing+=("BWOC_JIRA_TOKEN")
  [[ -n "$BASE_URL" ]] || missing+=("BWOC_JIRA_BASE_URL")
  if (( ${#missing[@]} > 0 )); then
    local list="${missing[*]}"
    printf '%s\n' "$PLUGIN: missing Jira credentials: ${list// /, } — set the BWOC_JIRA_* env vars (never commit the token)." >&2
    emit_error_json "auth_missing" "missing Jira credentials: ${list// /, }"
    exit 2
  fi
  BASE_URL="${BASE_URL%/}" # normalize a trailing slash
}

# ── HTTP with retry/backoff + error-class mapping ──────────────────────────
#
# Temp files for header dump + body so we can read Retry-After on 429.

TMPDIR_PLUGIN="$(mktemp -d "${TMPDIR:-/tmp}/jira-cloud-rest.XXXXXX")"
HDR_FILE="$TMPDIR_PLUGIN/headers"
BODY_FILE="$TMPDIR_PLUGIN/body"
ERR_FILE="$TMPDIR_PLUGIN/curlerr"
trap 'rm -rf "$TMPDIR_PLUGIN"' EXIT

# _curl_retry <curl args...> — standard auth/format flags are prepended.
# Honors HTTP 429 Retry-After (exponential fallback) up to MAX_ATTEMPTS.
# Sets HTTP_STATUS (0 on transport failure) + HTTP_BODY.
_curl_retry() {
  local attempt=0 code ra
  while :; do
    attempt=$((attempt + 1))
    : >"$HDR_FILE"; : >"$BODY_FILE"; : >"$ERR_FILE"
    code="$(curl -sS -u "$EMAIL:$TOKEN" \
      -H "Accept: application/json" \
      -D "$HDR_FILE" -o "$BODY_FILE" -w '%{http_code}' \
      "$@" 2>"$ERR_FILE")" || code=""
    if [[ -z "$code" ]]; then
      HTTP_STATUS=0
      HTTP_BODY="$(cat "$ERR_FILE" 2>/dev/null || true)"
      return 0
    fi
    HTTP_STATUS="$code"
    HTTP_BODY="$(cat "$BODY_FILE" 2>/dev/null || true)"
    if [[ "$code" == "429" && $attempt -lt $MAX_ATTEMPTS ]]; then
      ra="$(awk 'tolower($1)=="retry-after:"{gsub(/\r/,"",$2); print $2; exit}' "$HDR_FILE")"
      [[ "$ra" =~ ^[0-9]+$ ]] || ra=$((attempt * attempt)) # jittered-ish fallback
      sleep "$ra"
      continue
    fi
    return 0
  done
}

# classify_status <verb> — return 0 on 2xx; otherwise emit a clear diagnostic
# and exit. Distinguishes the error classes the BWOC-40 note §3 calls out.
classify_status() {
  local verb="$1"
  case "$HTTP_STATUS" in
    2*) return 0 ;;
    0)
      printf '%s\n' "$PLUGIN $verb: network/transport error reaching Jira: $(printf '%s' "$HTTP_BODY" | head -c 300)" >&2
      exit 6 ;;
    401|403)
      printf '%s\n' "$PLUGIN $verb: authentication failed (HTTP $HTTP_STATUS) — rotate BWOC_JIRA_TOKEN or check BWOC_JIRA_EMAIL. Not a sync conflict." >&2
      exit 3 ;;
    404)
      printf '%s\n' "$PLUGIN $verb: not found (HTTP 404) — possible mapping drift (issue moved/deleted). Surfaced to operator; never silently recreated." >&2
      exit 5 ;;
    429)
      printf '%s\n' "$PLUGIN $verb: rate limited (HTTP 429) after $MAX_ATTEMPTS attempts — retryable; back off and retry later." >&2
      exit 4 ;;
    *)
      printf '%s\n' "$PLUGIN $verb: Jira returned HTTP $HTTP_STATUS: $(printf '%s' "$HTTP_BODY" | head -c 300)" >&2
      exit 6 ;;
  esac
}

# ── Verb: query — project-scoped JQL read (GET /rest/api/3/search) ─────────

do_query() {
  require_auth
  local jql start_at max_results
  jql="$(printf '%s' "$REQUEST" | jq -r '.jql // empty')"
  start_at="$(printf '%s' "$REQUEST" | jq -r '.start_at // 0')"
  max_results="$(printf '%s' "$REQUEST" | jq -r '.max_results // 50')"

  if [[ -z "$jql" ]]; then
    printf '%s\n' "$PLUGIN query: empty JQL" >&2
    emit_error_json "bad_args" "empty JQL"
    exit 2
  fi
  [[ "$start_at"    =~ ^[0-9]+$ ]] || start_at=0
  [[ "$max_results" =~ ^[0-9]+$ ]] || max_results=50
  if (( max_results > MAX_PAGE )); then max_results=$MAX_PAGE; fi # never unbounded

  # Project scoping (least-leakage): if a project is configured and the JQL does
  # not already constrain one, wrap it. Best-effort in v0.1.0 — see SPEC §JQL.
  if [[ -n "${BWOC_JIRA_PROJECT:-}" ]] && ! printf '%s' "$jql" | grep -qiE 'project[[:space:]]*='; then
    jql="project = \"${BWOC_JIRA_PROJECT}\" AND (${jql})"
  fi

  _curl_retry -G "${BASE_URL}${API_BASE}/search" \
    --data-urlencode "jql=${jql}" \
    --data-urlencode "startAt=${start_at}" \
    --data-urlencode "maxResults=${max_results}" \
    --data-urlencode "fields=summary,status,assignee"
  classify_status "query"

  # Project the response into the Issue Mapping shape (PLUGINS.en.md).
  printf '%s' "$HTTP_BODY" | jq '{
    ok: true,
    operation: "query",
    total: (.total // 0),
    start_at: (.startAt // 0),
    max_results: (.maxResults // 0),
    issues: [ .issues[]? | {
      issue_key: .key,
      project: ((.key | split("-"))[0]),
      summary: (.fields.summary // null),
      status: (.fields.status.name // null),
      assignee: (.fields.assignee.accountId // .fields.assignee.emailAddress // null)
    } ]
  }'
}

# ── Verb: transition — gated status write (idempotent) ─────────────────────

do_transition() {
  require_auth
  local issue to_status
  issue="$(printf '%s' "$REQUEST" | jq -r '.issue // empty')"
  to_status="$(printf '%s' "$REQUEST" | jq -r '.to_status // empty')"
  if [[ -z "$issue" || -z "$to_status" ]]; then
    printf '%s\n' "$PLUGIN transition: both .issue and .to_status are required" >&2
    emit_error_json "bad_args" "issue and to_status required"
    exit 2
  fi

  # Idempotency guard: if already in the target status, this is a no-op success.
  _curl_retry "${BASE_URL}${API_BASE}/issue/${issue}?fields=status"
  classify_status "transition"
  local current
  current="$(printf '%s' "$HTTP_BODY" | jq -r '.fields.status.name // empty')"
  if [[ "$current" == "$to_status" ]]; then
    jq -n --arg issue "$issue" --arg st "$to_status" \
      '{ok:true, operation:"transition", issue:$issue, to_status:$st, transitioned:false, note:"already in target status (idempotent no-op)"}'
    return 0
  fi

  # Resolve the transition id whose target status matches the request.
  _curl_retry "${BASE_URL}${API_BASE}/issue/${issue}/transitions"
  classify_status "transition"
  local tid
  tid="$(printf '%s' "$HTTP_BODY" | jq -r --arg s "$to_status" \
    'first(.transitions[]? | select((.to.name == $s) or (.name == $s)) | .id) // empty')"
  if [[ -z "$tid" ]]; then
    printf '%s\n' "$PLUGIN transition: no workflow transition from '$current' to '$to_status' is available for $issue" >&2
    emit_error_json "no_transition" "no transition to '$to_status' available for $issue (from '$current')"
    exit 7
  fi

  # Apply. Replaying after a 429 backoff converges (the guard above short-circuits
  # the second attempt once the status has changed) — idempotent by construction.
  local payload
  payload="$(jq -n --arg id "$tid" '{transition:{id:$id}}')"
  _curl_retry -X POST -H "Content-Type: application/json" \
    --data "$payload" \
    "${BASE_URL}${API_BASE}/issue/${issue}/transitions"
  classify_status "transition"

  jq -n --arg issue "$issue" --arg st "$to_status" --arg tid "$tid" \
    '{ok:true, operation:"transition", issue:$issue, to_status:$st, transition_id:$tid, transitioned:true}'
}

# ── Verb: sync — structured skeleton over the ledger ───────────────────────
#
# v0.1.0 reference adapter: the per-field last-writer-wins resolution engine
# (BWOC-40 note §4) is structured but DEFERRED to the EPIC-6 sync engine. This
# adapter establishes the contract + auth + read path; it reports every mapped
# issue as a no-op rather than guessing a resolution it cannot yet compute
# safely. The CLI reads summary.{push,pull,noop,conflict}; the shape is final.

do_sync() {
  require_auth
  local dry_run ledger mapped dry_json
  dry_run="$(printf '%s' "$REQUEST" | jq -r '.dry_run // false')"
  ledger="${BWOC_WORKSPACE:-.}/.scrum/jira-sync.json"
  mapped=0
  if [[ -f "$ledger" ]]; then
    mapped="$(jq -r '(.issues // {}) | length' "$ledger" 2>/dev/null || echo 0)"
  fi
  [[ "$mapped" =~ ^[0-9]+$ ]] || mapped=0
  dry_json=false
  if [[ "$dry_run" == "true" ]]; then dry_json=true; fi

  jq -n --argjson noop "$mapped" --argjson dry "$dry_json" \
    '{ok:true, operation:"sync", dry_run:$dry,
      summary:{push:0, pull:0, noop:$noop, conflict:0},
      note:"v0.1.0 reference adapter — bidirectional field-level resolution deferred to the EPIC-6 sync engine; mapped issues reported as no-op, no writes performed."}'
}

# ── Dispatch ───────────────────────────────────────────────────────────────

case "$OPERATION" in
  query)      do_query ;;
  transition) do_transition ;;
  sync)       do_sync ;;
  *)
    printf '%s\n' "$PLUGIN: unknown operation '$OPERATION' (expected query | transition | sync)" >&2
    emit_error_json "unknown_operation" "unknown operation '$OPERATION'"
    exit 2 ;;
esac
