#!/usr/bin/env bash
#
# gws-slides — gws/gws-slides plugin entry (BWOC-354 series).
#
# A per-service plugin of the `gws` kind with a WRITE path. Reads a Google
# Slides presentation (presentations.get) and edits it via presentations.
# batchUpdate (the general write verb) plus the convenience replace-all-text.
# Reads project into the normative Google Presentation shape (docs/en/PLUGINS.en.md
# §"Workspace Resource Schema"). Requires the `presentations` OAuth scope.
#
# The write verbs (batch-update, replace-all-text) are gated by the operator-
# confirm gate at the `bwoc gws slides` CLI boundary (PLUGINS §Write verbs). This
# plugin executes when invoked and never re-implements nor bypasses that gate.
#
# Sources the OAuth credential helpers from the sibling gws/gws-auth plugin (the
# gcloud-* family shape). Sourcing is BASH_SOURCE-guarded on the sibling side.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"get","presentation_id":"1AbC"}
#                          {"operation":"batch-update","presentation_id":"1AbC","requests":[{...}]}
#                          {"operation":"replace-all-text","presentation_id":"1AbC","find":"X","replace":"Y","match_case":false}
#   BWOC_GWS_OPERATION     fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (token file resolution)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#   BWOC_GWS_TOKEN         the OAuth2 access token — SECRET (inherited env)
#
# On success: exit 0 + a single JSON object on stdout. On error: a human message
# on stderr + non-zero exit.
#
# Security (Sila — Adinnaadana):
#   Never reads or prints the token value. Hands the request to the sibling's
#   gws_curl and projects Slides' JSON response — never the credential.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gws_slides_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gws_slides_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gws-auth/gws.sh")
  fi
  candidates+=("$(_gws_slides_self_dir)/../gws-auth/gws.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gws_slides_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gws-slides: sibling helpers gws/gws-auth/gws.sh not found — install gws/gws-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

PLUGIN="gws-slides"
API_BASE="https://slides.googleapis.com/v1/presentations"

# ── stdin + dependencies ───────────────────────────────────────────────────

for cmd in jq curl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN: required command '$cmd' not found on PATH — install it, then retry." >&2
    exit 1
  fi
done

REQUEST="$(cat || true)"
req() { printf '%s' "$REQUEST" | jq -r "$1" 2>/dev/null || true; }
reqjson() { printf '%s' "$REQUEST" | jq -c "$1" 2>/dev/null || true; }

OPERATION=""
if [[ -n "$REQUEST" ]]; then OPERATION="$(req '.operation // empty')"; fi
if [[ -z "$OPERATION" ]]; then OPERATION="${BWOC_GWS_OPERATION:-}"; fi
if [[ -z "$OPERATION" ]]; then
  printf '%s\n' "$PLUGIN: no operation (set BWOC_GWS_OPERATION or pipe a JSON request carrying .operation)" >&2
  exit 2
fi

# Presentation id: opaque [A-Za-z0-9_-], no leading hyphen, 1..=512.
_require_presentation_id() {
  local id="$1"
  if [[ -z "$id" ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: .presentation_id is required" >&2
    exit 2
  fi
  if (( ${#id} > 512 )) || [[ ! "$id" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]*$ ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: invalid presentation_id '$id' (expected 1..=512 chars of [A-Za-z0-9_-], no leading hyphen)" >&2
    exit 2
  fi
}

# ── Verb: get — presentations.get → Presentation entry (+ slide count) ─────

do_get() {
  gws_assert_token || exit 2
  local id
  id="$(req '.presentation_id // empty')"
  _require_presentation_id "$id"

  gws_curl -G "${API_BASE}/${id}" \
    --data-urlencode "fields=presentationId,title,slides(objectId)"
  gws_classify_status "get" "Presentation '${id}'"

  printf '%s' "$HTTP_BODY" | jq '
    { ok: true, plugin: "gws-slides", operation: "get",
      presentation: {
        presentation_id: .presentationId,
        title: (.title // ""),
        slide_count: ((.slides // []) | length),
        web_view_link: ("https://docs.google.com/presentation/d/" + (.presentationId // "") + "/edit")
      },
      slide_ids: [ (.slides // [])[] | .objectId ] }'
}

# ── batchUpdate POST — shared by batch-update and replace-all-text ─────────
_post_batch_update() {
  local presentation_id="$1" requests="$2" op="$3" body tmp
  body="$(jq -cn --argjson r "$requests" '{requests: $r}')"
  tmp="$(mktemp "${TMPDIR:-/tmp}/gws-slides.XXXXXX")"
  printf '%s' "$body" >"$tmp"

  gws_curl -X POST "${API_BASE}/${presentation_id}:batchUpdate" \
    -H "Content-Type: application/json" \
    --data-binary "@${tmp}"
  local status=$HTTP_STATUS
  rm -f "$tmp"
  HTTP_STATUS=$status
  gws_classify_status "$op" "Presentation '${presentation_id}'"

  printf '%s' "$HTTP_BODY" | jq --arg op "$op" --arg id "$presentation_id" --argjson n "$(printf '%s' "$requests" | jq 'length')" '
    { ok: true, plugin: "gws-slides", operation: $op,
      presentation_id: (.presentationId // $id),
      requests_applied: $n,
      occurrences_changed: ([ (.replies // [])[] | .replaceAllText.occurrencesChanged // empty ] | add // 0),
      replies: (.replies // []) }'
}

do_batch_update() {
  gws_assert_token || exit 2
  local presentation_id requests
  presentation_id="$(req '.presentation_id // empty')"
  _require_presentation_id "$presentation_id"
  requests="$(reqjson '.requests // empty')"
  if [[ -z "$requests" || "$requests" == "null" ]]; then
    printf '%s\n' "$PLUGIN batch-update: .requests is required (a JSON array of Slides API request objects)" >&2
    exit 2
  fi
  if [[ "$(printf '%s' "$requests" | jq -r 'if type=="array" then "ok" else "no" end' 2>/dev/null)" != "ok" ]]; then
    printf '%s\n' "$PLUGIN batch-update: .requests must be a JSON array of Slides API request objects" >&2
    exit 2
  fi
  if [[ "$(printf '%s' "$requests" | jq 'length')" -eq 0 ]]; then
    printf '%s\n' "$PLUGIN batch-update: .requests is empty — nothing to apply" >&2
    exit 2
  fi
  _post_batch_update "$presentation_id" "$requests" "batch-update"
}

do_replace_all_text() {
  gws_assert_token || exit 2
  local presentation_id find replace match_case
  presentation_id="$(req '.presentation_id // empty')"
  _require_presentation_id "$presentation_id"
  find="$(req '.find // empty')"
  if [[ -z "$find" ]]; then
    printf '%s\n' "$PLUGIN replace-all-text: .find is required (the text to match)" >&2
    exit 2
  fi
  replace="$(req '.replace // ""')"
  match_case="$(req '.match_case // false')"
  [[ "$match_case" == "true" ]] || match_case="false"

  local requests
  requests="$(jq -cn --arg f "$find" --arg r "$replace" --argjson mc "$match_case" '
    [ { replaceAllText: { containsText: { text: $f, matchCase: $mc }, replaceText: $r } } ]')"
  _post_batch_update "$presentation_id" "$requests" "replace-all-text"
}

# ── Dispatch ───────────────────────────────────────────────────────────────

case "$OPERATION" in
  get)               do_get ;;
  batch-update)      do_batch_update ;;
  replace-all-text)  do_replace_all_text ;;
  *)
    printf '%s\n' "$PLUGIN: unknown operation '$OPERATION' (expected get | batch-update | replace-all-text)" >&2
    exit 2 ;;
esac
