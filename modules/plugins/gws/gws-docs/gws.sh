#!/usr/bin/env bash
#
# gws-docs — gws/gws-docs plugin entry (BWOC-354).
#
# A per-service plugin of the `gws` kind — the first with an in-place WRITE path.
# Reads a Google Doc (documents.get) and edits it via documents.batchUpdate (the
# general write verb) plus the convenience replace-all-text. Reads project into
# the normative Google Doc shape (docs/en/PLUGINS.en.md §"Workspace Resource
# Schema"). Requires the `documents` OAuth scope (read + write).
#
# The write verbs (batch-update, replace-all-text) are gated by the operator-
# confirm gate at the `bwoc gws docs` CLI boundary (PLUGINS §Write verbs). This
# plugin executes when invoked and never re-implements nor bypasses that gate —
# by the time gws.sh runs, the operator has already confirmed (or passed --yes).
#
# Sources the OAuth credential helpers from the sibling gws/gws-auth plugin so
# the Bearer-auth + rate-limit + refresh implementation lives exactly once
# (the gcloud-* family shape). Sourcing is BASH_SOURCE-guarded on the sibling
# side, so importing the helpers does not run the gws-auth dispatcher.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"get","document_id":"1AbC_dEf"}
#                          {"operation":"batch-update","document_id":"1AbC","requests":[{...}]}
#                          {"operation":"replace-all-text","document_id":"1AbC","find":"X","replace":"Y","match_case":false}
#   BWOC_GWS_OPERATION     fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (token file resolution)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#                          (used to find ../gws-auth/gws.sh)
#   BWOC_GWS_TOKEN         the OAuth2 access token — SECRET (inherited env)
#
# On success: exit 0 + a single JSON object on stdout. On error: a human
# message on stderr + non-zero exit (the CLI surfaces it).
#
# Security (Sila — Adinnaadana):
#   Never reads or prints the token value. Hands the request to the sibling's
#   gws_curl (which sets the Bearer header) and projects Docs' JSON response —
#   never the credential — into the output.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gws_docs_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gws_docs_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gws-auth/gws.sh")
  fi
  candidates+=("$(_gws_docs_self_dir)/../gws-auth/gws.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gws_docs_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gws-docs: sibling helpers gws/gws-auth/gws.sh not found — install gws/gws-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

# The sourced helpers set PLUGIN="gws-auth"; override AFTER sourcing.
PLUGIN="gws-docs"
API_BASE="https://docs.googleapis.com/v1"
TEXT_CAP=20000   # chars of extracted body text returned by `get` (bounded pull)

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

# Validate a Google Docs document id: opaque [A-Za-z0-9_-], no leading hyphen, so
# a crafted id can never inject a path segment or query into the request URL.
_require_document_id() {
  local id="$1"
  if [[ -z "$id" ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: .document_id is required (pass {\"document_id\":\"<id>\"})" >&2
    exit 2
  fi
  # Match the CLI pre-check (is_valid_resource_id): 1..=512 chars of
  # [A-Za-z0-9_-], no LEADING hyphen (so a crafted id can never inject a curl
  # option or a path/query segment into the request URL).
  if (( ${#id} > 512 )) || [[ ! "$id" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]*$ ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: invalid document_id '$id' (expected 1..=512 chars of [A-Za-z0-9_-], no leading hyphen)" >&2
    exit 2
  fi
}

# ── Verb: get — documents.get → normative Google Doc entry (+ body text) ────

do_get() {
  gws_assert_token || exit 2
  local document_id
  document_id="$(req '.document_id // empty')"
  _require_document_id "$document_id"

  gws_curl -G "${API_BASE}/documents/${document_id}"
  gws_classify_status "get" "Google Doc '${document_id}'"

  # Project: normative Doc entry + a bounded plain-text extraction of the body
  # (concatenate every paragraph textRun's content). Optional web_view_link is
  # synthesized from the id (documents.get carries no webViewLink field).
  printf '%s' "$HTTP_BODY" | jq --argjson cap "$TEXT_CAP" '
    def body_text:
      [ (.body.content // [])[]
        | (.paragraph.elements // [])[]
        | (.textRun.content // "") ] | join("");
    { ok: true, plugin: "gws-docs", operation: "get",
      document: {
        document_id: .documentId,
        title: (.title // ""),
        revision_id: (.revisionId // ""),
        web_view_link: ("https://docs.google.com/document/d/" + (.documentId // "") + "/edit")
      },
      text: (body_text | .[0:$cap]),
      text_truncated: ((body_text | length) > $cap)
    }'
}

# ── batchUpdate POST — shared by batch-update and replace-all-text ──────────
# Posts {"requests": <arr>} to documents.{id}:batchUpdate and returns the
# projected write summary. $1 = document_id, $2 = requests JSON array,
# $3 = operation label for the envelope.
_post_batch_update() {
  local document_id="$1" requests="$2" op="$3" body tmp
  body="$(jq -cn --argjson r "$requests" '{requests: $r}')"
  tmp="$(mktemp "${TMPDIR:-/tmp}/gws-docs.XXXXXX")"
  printf '%s' "$body" >"$tmp"

  gws_curl -X POST "${API_BASE}/documents/${document_id}:batchUpdate" \
    -H "Content-Type: application/json" \
    --data-binary "@${tmp}"
  local status=$HTTP_STATUS
  rm -f "$tmp"
  HTTP_STATUS=$status
  gws_classify_status "$op" "Google Doc '${document_id}'"

  # replies[] mirror the requests; occurrences changed (replaceAllText) surfaced
  # when present. Never echo the document body back — this is a write receipt.
  printf '%s' "$HTTP_BODY" | jq --arg op "$op" --arg id "$document_id" --argjson n "$(printf '%s' "$requests" | jq 'length')" '
    { ok: true, plugin: "gws-docs", operation: $op,
      document_id: $id,
      # batchUpdate only returns a revision under writeControl (when the request
      # asked for it); absent → empty string. documentId is NOT a revision id.
      revision_id: (.writeControl.requiredRevisionId // ""),
      requests_applied: $n,
      occurrences_changed: ([ (.replies // [])[] | .replaceAllText.occurrencesChanged // empty ] | add // 0),
      replies: (.replies // []) }'
}

# ── Verb: batch-update — documents.batchUpdate (general write) ──────────────

do_batch_update() {
  gws_assert_token || exit 2
  local document_id requests
  document_id="$(req '.document_id // empty')"
  _require_document_id "$document_id"
  requests="$(reqjson '.requests // empty')"
  if [[ -z "$requests" || "$requests" == "null" ]]; then
    printf '%s\n' "$PLUGIN batch-update: .requests is required (a JSON array of Docs API request objects)" >&2
    exit 2
  fi
  if [[ "$(printf '%s' "$requests" | jq -r 'if type=="array" then "ok" else "no" end' 2>/dev/null)" != "ok" ]]; then
    printf '%s\n' "$PLUGIN batch-update: .requests must be a JSON array of Docs API request objects" >&2
    exit 2
  fi
  if [[ "$(printf '%s' "$requests" | jq 'length')" -eq 0 ]]; then
    printf '%s\n' "$PLUGIN batch-update: .requests is empty — nothing to apply" >&2
    exit 2
  fi
  _post_batch_update "$document_id" "$requests" "batch-update"
}

# ── Verb: replace-all-text — convenience over a single replaceAllText req ───

do_replace_all_text() {
  gws_assert_token || exit 2
  local document_id find replace match_case
  document_id="$(req '.document_id // empty')"
  _require_document_id "$document_id"
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
  _post_batch_update "$document_id" "$requests" "replace-all-text"
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
