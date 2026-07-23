#!/usr/bin/env bash
#
# accounting-api — workflow/accounting-api plugin entry.
#
# Adapter over the Bemind Accounting Open API (v2.3.2,
# https://accounting.bemind.tech/api/v1). Reads financial reports and records
# purchases/expenses. Every write auto-posts a double-entry GL entry server-side.
#
# Auth: an operator-supplied API key (bound to one seller), resolved at runtime
# from BWOC_ACCOUNTING_KEY (env) or <workspace>/.bwoc/secrets/accounting-key
# (file, gitignored). NEVER committed, NEVER printed. A User-Agent header is
# REQUIRED (the edge returns Cloudflare 1010 without one).
#
# The write verbs' operator-confirm gate belongs at the `bwoc accounting` CLI
# (a follow-up slice); this plugin executes when invoked and never re-implements
# nor bypasses that gate.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"report","report":"pnl","params":{"from":"2026-01-01","to":"2026-03-31"}}
#                          {"operation":"bill-create","type":"bill"}
#                          {"operation":"bill-update","document_id":"PI-123","payload":{"date":"2026-07-24","supplier":{"name":"ACME"},"items":[{"description":"x","quantity":1,"unit":"ea","unitPrice":100}]}}
#                          {"operation":"expense-create","payload":{"date":"2026-07-24","description":"taxi","amount":150}}
#   BWOC_WORKSPACE         absolute workspace root (secrets-file resolution)
#   BWOC_ACCOUNTING_KEY    the API key — SECRET (inherited env)
#
# On success: exit 0 + a single JSON object on stdout. On error: a human message
# on stderr + non-zero exit.

set -euo pipefail

PLUGIN="accounting-api"
API_BASE="https://accounting.bemind.tech/api/v1"
KEY_REL=".bwoc/secrets/accounting-key"
UA="bwoc-accounting-api/0.1.0 (+https://github.com/bemindlabs/BWOC-Framework)"
# Report names the API exposes under /reports/<name> (v2.3.2). A crafted name
# can never leave this set, so it can't inject a path segment.
REPORTS="pnl balance-sheet cashflow trial-balance vat wht ap-aging ar-aging expenses sales-by-channel mrr product-margin asset-register"

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
if [[ -z "$OPERATION" ]]; then OPERATION="${BWOC_ACCOUNTING_OPERATION:-}"; fi
if [[ -z "$OPERATION" ]]; then
  printf '%s\n' "$PLUGIN: no operation (pipe a JSON request carrying .operation)" >&2
  exit 2
fi

# ── credential resolution (never printed) ──────────────────────────────────
_resolve_key() {
  if [[ -n "${BWOC_ACCOUNTING_KEY:-}" ]]; then printf '%s' "$BWOC_ACCOUNTING_KEY"; return 0; fi
  local f="${BWOC_WORKSPACE:-}/$KEY_REL"
  if [[ -n "${BWOC_WORKSPACE:-}" && -r "$f" ]]; then
    # first non-empty line, trimmed; the file holds only the key
    sed -n '1p' "$f" | tr -d '[:space:]'
    return 0
  fi
  return 0
}
_assert_key() {
  if [[ -z "$(_resolve_key)" ]]; then
    printf '%s\n' "$PLUGIN: no API key — set BWOC_ACCOUNTING_KEY or write \$BWOC_WORKSPACE/$KEY_REL (chmod 600). Never commit the key." >&2
    exit 2
  fi
}

# acct_curl <curl args...> — authenticated request. Sets HTTP_STATUS + HTTP_BODY.
acct_curl() {
  local key tmp_b tmp_e code
  key="$(_resolve_key)"
  tmp_b="$(mktemp "${TMPDIR:-/tmp}/acct-b.XXXXXX")"
  tmp_e="$(mktemp "${TMPDIR:-/tmp}/acct-e.XXXXXX")"
  code="$(curl -sS \
    -H "Authorization: Bearer ${key}" \
    -H "User-Agent: ${UA}" \
    -H "Accept: application/json" \
    -o "$tmp_b" -w '%{http_code}' \
    "$@" 2>"$tmp_e")" || code=""
  if [[ -z "$code" ]]; then
    HTTP_STATUS=0
    HTTP_BODY="$(cat "$tmp_e" 2>/dev/null || true)"
  else
    HTTP_STATUS="$code"
    HTTP_BODY="$(cat "$tmp_b" 2>/dev/null || true)"
  fi
  rm -f "$tmp_b" "$tmp_e"
}

# acct_classify <verb> [resource] — 0 on 2xx; else clear diagnostic + exit.
acct_classify() {
  local verb="$1" resource="${2:-resource}"
  case "$HTTP_STATUS" in
    2*) return 0 ;;
    0)   printf '%s\n' "$PLUGIN $verb: network/transport error: $(printf '%s' "$HTTP_BODY" | head -c 200)" >&2; exit 6 ;;
    401) printf '%s\n' "$PLUGIN $verb: authentication failed (HTTP 401) — the API key is missing/invalid/revoked." >&2; exit 3 ;;
    403) printf '%s\n' "$PLUGIN $verb: forbidden (HTTP 403) — the key lacks the required scope for $resource (e.g. reports:read / purchases:write / expenses:write)." >&2; exit 3 ;;
    404) printf '%s\n' "$PLUGIN $verb: $resource not found (HTTP 404)." >&2; exit 5 ;;
    429) printf '%s\n' "$PLUGIN $verb: rate limited (HTTP 429) — back off and retry." >&2; exit 4 ;;
    *)   printf '%s\n' "$PLUGIN $verb: API returned HTTP $HTTP_STATUS: $(printf '%s' "$HTTP_BODY" | head -c 200)" >&2; exit 6 ;;
  esac
}

# Validate an opaque document id used in a path segment.
_require_document_id() {
  local id="$1"
  if [[ -z "$id" ]]; then printf '%s\n' "$PLUGIN $OPERATION: .document_id is required" >&2; exit 2; fi
  if (( ${#id} > 128 )) || [[ ! "$id" =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]]; then
    printf '%s\n' "$PLUGIN $OPERATION: invalid document_id '$id' (1..=128 chars of [A-Za-z0-9_-], no leading hyphen)" >&2
    exit 2
  fi
}

# ── Verb: report (read) — GET /reports/<name> ──────────────────────────────
do_report() {
  _assert_key
  local name
  name="$(req '.report // empty')"
  if [[ -z "$name" ]] || ! printf '%s\n' $REPORTS | grep -qx "$name"; then
    printf '%s\n' "$PLUGIN report: .report must be one of: $REPORTS" >&2
    exit 2
  fi
  # Optional flat query params → --data-urlencode k=v (curl -G appends to the URL).
  local args=(-G "${API_BASE}/reports/${name}")
  local kv
  while IFS= read -r kv; do
    [[ -n "$kv" ]] && args+=(--data-urlencode "$kv")
  done < <(printf '%s' "$REQUEST" | jq -r '(.params // {}) | to_entries[] | "\(.key)=\(.value)"' 2>/dev/null || true)

  acct_curl "${args[@]}"
  acct_classify "report" "report '${name}'"
  printf '%s' "$HTTP_BODY" | jq --arg r "$name" '{ ok: true, plugin: "accounting-api", operation: "report", report: $r, data: (.data // .) }'
}

# POST/PATCH JSON helper — $1 method, $2 url, $3 body JSON, $4 verb, $5 resource
_write_json() {
  local method="$1" url="$2" body="$3" verb="$4" resource="$5" tmp
  tmp="$(mktemp "${TMPDIR:-/tmp}/acct-w.XXXXXX")"
  printf '%s' "$body" >"$tmp"
  acct_curl -X "$method" "$url" -H "Content-Type: application/json" --data-binary "@${tmp}"
  local status=$HTTP_STATUS
  rm -f "$tmp"
  HTTP_STATUS=$status
  acct_classify "$verb" "$resource"
}

# ── Verb: bill-create (write) — POST /purchase-docs {type} → draft ─────────
do_bill_create() {
  _assert_key
  local type
  type="$(req '.type // "bill"')"
  case "$type" in bill|purchase_order|goods_receipt) ;; *)
    printf '%s\n' "$PLUGIN bill-create: .type must be bill | purchase_order | goods_receipt" >&2; exit 2 ;;
  esac
  _write_json POST "${API_BASE}/purchase-docs" "$(jq -cn --arg t "$type" '{type: $t}')" "bill-create" "purchase-docs"
  printf '%s' "$HTTP_BODY" | jq '(.data // .) as $d | { ok: true, plugin: "accounting-api", operation: "bill-create", document_id: ($d.id // ""), number: ($d.number // ""), type: ($d.type // "") }'
}

# ── Verb: bill-update (write) — PATCH /purchase-docs/{id} {payload} ────────
do_bill_update() {
  _assert_key
  local id payload
  id="$(req '.document_id // empty')"; _require_document_id "$id"
  payload="$(reqjson '.payload // empty')"
  if [[ -z "$payload" || "$payload" == "null" || "$(printf '%s' "$payload" | jq -r 'type')" != "object" ]]; then
    printf '%s\n' "$PLUGIN bill-update: .payload is required (a JSON object: date, supplier, items, vat, …)" >&2
    exit 2
  fi
  _write_json PATCH "${API_BASE}/purchase-docs/${id}" "$payload" "bill-update" "purchase-doc '${id}'"
  printf '%s' "$HTTP_BODY" | jq --arg id "$id" '(.data // .) as $d | { ok: true, plugin: "accounting-api", operation: "bill-update", document_id: ($d.id // $id), number: ($d.number // ""), status: ($d.status // "") }'
}

# ── Verb: expense-create (write) — POST /expenses {payload} ────────────────
do_expense_create() {
  _assert_key
  local payload
  payload="$(reqjson '.payload // empty')"
  if [[ -z "$payload" || "$payload" == "null" || "$(printf '%s' "$payload" | jq -r 'type')" != "object" ]]; then
    printf '%s\n' "$PLUGIN expense-create: .payload is required (a JSON object: date, description, amount, …)" >&2
    exit 2
  fi
  _write_json POST "${API_BASE}/expenses" "$payload" "expense-create" "expenses"
  printf '%s' "$HTTP_BODY" | jq '(.data // .) as $d | { ok: true, plugin: "accounting-api", operation: "expense-create", expense_id: ($d.id // ""), number: ($d.number // "") }'
}

case "$OPERATION" in
  report)          do_report ;;
  bill-create)     do_bill_create ;;
  bill-update)     do_bill_update ;;
  expense-create)  do_expense_create ;;
  *)
    printf '%s\n' "$PLUGIN: unknown operation '$OPERATION' (expected report | bill-create | bill-update | expense-create)" >&2
    exit 2 ;;
esac
