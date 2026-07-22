#!/usr/bin/env bash
#
# gws-sheets — gws/gws-sheets plugin entry (BWOC-354 series).
#
# A per-service plugin of the `gws` kind with a WRITE path. Reads spreadsheet
# metadata (spreadsheets.get) and cell ranges (spreadsheets.values.get), and
# edits values via spreadsheets.values.update / spreadsheets.values.append. Reads project
# into the normative Google Spreadsheet shape (docs/en/PLUGINS.en.md §"Workspace
# Resource Schema"). Requires the `spreadsheets` OAuth scope (read + write).
#
# The write verbs (values-update, values-append) are gated by the operator-
# confirm gate at the `bwoc gws sheets` CLI boundary (PLUGINS §Write verbs). This
# plugin executes when invoked and never re-implements nor bypasses that gate.
#
# Sources the OAuth credential helpers from the sibling gws/gws-auth plugin (the
# gcloud-* family shape). Sourcing is BASH_SOURCE-guarded on the sibling side.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"get","spreadsheet_id":"1AbC"}
#                          {"operation":"values-get","spreadsheet_id":"1AbC","range":"Sheet1!A1:B2"}
#                          {"operation":"values-update","spreadsheet_id":"1AbC","range":"Sheet1!A1","values":[["x"]]}
#                          {"operation":"values-append","spreadsheet_id":"1AbC","range":"Sheet1!A1","values":[["x"]]}
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
#   gws_curl and projects Sheets' JSON response — never the credential.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gws_sheets_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gws_sheets_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gws-auth/gws.sh")
  fi
  candidates+=("$(_gws_sheets_self_dir)/../gws-auth/gws.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gws_sheets_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gws-sheets: sibling helpers gws/gws-auth/gws.sh not found — install gws/gws-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

PLUGIN="gws-sheets"
API_BASE="https://sheets.googleapis.com/v4/spreadsheets"
VALUE_INPUT="USER_ENTERED"   # let Sheets parse types/formulas (vs RAW)

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
urlenc() { jq -rn --arg s "$1" '$s|@uri'; }

OPERATION=""
if [[ -n "$REQUEST" ]]; then OPERATION="$(req '.operation // empty')"; fi
if [[ -z "$OPERATION" ]]; then OPERATION="${BWOC_GWS_OPERATION:-}"; fi
if [[ -z "$OPERATION" ]]; then
  printf '%s\n' "$PLUGIN: no operation (set BWOC_GWS_OPERATION or pipe a JSON request carrying .operation)" >&2
  exit 2
fi

# Spreadsheet id: opaque [A-Za-z0-9_-], no leading hyphen, 1..=512.
_require_spreadsheet_id() {
  local id="$1"
  if [[ -z "$id" ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: .spreadsheet_id is required" >&2
    exit 2
  fi
  if (( ${#id} > 512 )) || [[ ! "$id" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]*$ ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: invalid spreadsheet_id '$id' (expected 1..=512 chars of [A-Za-z0-9_-], no leading hyphen)" >&2
    exit 2
  fi
}

# A1 range: alphanumerics + ! : $ ' . space, 1..=512, no control/'/' — enough for
# A1 notation ("Sheet1!A1:B2", "'My Sheet'!A:A") without opening a path segment.
_require_range() {
  local r="$1"
  if [[ -z "$r" ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: .range is required (A1 notation, e.g. Sheet1!A1:B2)" >&2
    exit 2
  fi
  if (( ${#r} > 512 )) || [[ "$r" == *"/"* ]] || [[ ! "$r" =~ ^[A-Za-z0-9_\!\:\$\'\.\ -]+$ ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: invalid range '$r' (A1 notation only)" >&2
    exit 2
  fi
}

# Require .values to be a 2-D array (array of row arrays) for the write verbs.
_require_values() {
  local v="$1"
  if [[ -z "$v" || "$v" == "null" ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: .values is required (a 2-D JSON array, e.g. [[\"a\",\"b\"]])" >&2
    exit 2
  fi
  if [[ "$(printf '%s' "$v" | jq -r 'if type=="array" and length>0 and (all(.[]; type=="array")) then "ok" else "no" end' 2>/dev/null)" != "ok" ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: .values must be a non-empty 2-D JSON array (array of row arrays)" >&2
    exit 2
  fi
}

# ── Verb: get — spreadsheets.get → Spreadsheet entry (+ tab list) ──────────

do_get() {
  gws_assert_token || exit 2
  local id
  id="$(req '.spreadsheet_id // empty')"
  _require_spreadsheet_id "$id"

  gws_curl -G "${API_BASE}/${id}" \
    --data-urlencode "fields=spreadsheetId,properties(title),sheets(properties(sheetId,title,index))"
  gws_classify_status "get" "Spreadsheet '${id}'"

  printf '%s' "$HTTP_BODY" | jq '
    { ok: true, plugin: "gws-sheets", operation: "get",
      spreadsheet: {
        spreadsheet_id: .spreadsheetId,
        title: (.properties.title // ""),
        sheet_count: ((.sheets // []) | length),
        web_view_link: ("https://docs.google.com/spreadsheets/d/" + (.spreadsheetId // "") + "/edit")
      },
      sheets: [ (.sheets // [])[] | { sheet_id: .properties.sheetId, title: .properties.title, index: .properties.index } ] }'
}

# ── Verb: values-get — spreadsheets.values.get → a value grid ──────────────

do_values_get() {
  gws_assert_token || exit 2
  local id range enc
  id="$(req '.spreadsheet_id // empty')"; _require_spreadsheet_id "$id"
  range="$(req '.range // empty')"; _require_range "$range"
  enc="$(urlenc "$range")"

  gws_curl -G "${API_BASE}/${id}/values/${enc}"
  gws_classify_status "values-get" "range '${range}' of Spreadsheet '${id}'"

  printf '%s' "$HTTP_BODY" | jq --arg id "$id" '
    { ok: true, plugin: "gws-sheets", operation: "values-get",
      spreadsheet_id: $id, range: (.range // ""),
      major_dimension: (.majorDimension // "ROWS"),
      values: (.values // []) }'
}

# ── values write POST/PUT — shared by update and append ────────────────────
# $1 = id, $2 = range, $3 = values JSON, $4 = "update"|"append"
_write_values() {
  local id="$1" range="$2" values="$3" mode="$4" enc body tmp method url
  enc="$(urlenc "$range")"
  body="$(jq -cn --argjson v "$values" '{values: $v}')"
  tmp="$(mktemp "${TMPDIR:-/tmp}/gws-sheets.XXXXXX")"
  printf '%s' "$body" >"$tmp"

  if [[ "$mode" == "append" ]]; then
    method="POST"
    url="${API_BASE}/${id}/values/${enc}:append?valueInputOption=${VALUE_INPUT}&insertDataOption=INSERT_ROWS"
  else
    method="PUT"
    url="${API_BASE}/${id}/values/${enc}?valueInputOption=${VALUE_INPUT}"
  fi

  gws_curl -X "$method" "$url" \
    -H "Content-Type: application/json" \
    --data-binary "@${tmp}"
  local status=$HTTP_STATUS
  rm -f "$tmp"
  HTTP_STATUS=$status
  gws_classify_status "values-$mode" "range '${range}' of Spreadsheet '${id}'"

  # update: top-level updated* fields. append: nested under .updates.
  printf '%s' "$HTTP_BODY" | jq --arg op "values-$mode" --arg id "$id" '
    (.updates // .) as $u
    | { ok: true, plugin: "gws-sheets", operation: $op, spreadsheet_id: $id,
        updated_range: ($u.updatedRange // ""),
        updated_rows: ($u.updatedRows // 0),
        updated_columns: ($u.updatedColumns // 0),
        updated_cells: ($u.updatedCells // 0) }'
}

do_values_update() {
  gws_assert_token || exit 2
  local id range values
  id="$(req '.spreadsheet_id // empty')"; _require_spreadsheet_id "$id"
  range="$(req '.range // empty')"; _require_range "$range"
  values="$(reqjson '.values // empty')"; _require_values "$values"
  _write_values "$id" "$range" "$values" "update"
}

do_values_append() {
  gws_assert_token || exit 2
  local id range values
  id="$(req '.spreadsheet_id // empty')"; _require_spreadsheet_id "$id"
  range="$(req '.range // empty')"; _require_range "$range"
  values="$(reqjson '.values // empty')"; _require_values "$values"
  _write_values "$id" "$range" "$values" "append"
}

# ── Dispatch ───────────────────────────────────────────────────────────────

case "$OPERATION" in
  get)            do_get ;;
  values-get)     do_values_get ;;
  values-update)  do_values_update ;;
  values-append)  do_values_append ;;
  *)
    printf '%s\n' "$PLUGIN: unknown operation '$OPERATION' (expected get | values-get | values-update | values-append)" >&2
    exit 2 ;;
esac
