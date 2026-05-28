#!/usr/bin/env bash
#
# figma-rest — Figma REST API integration adapter (BWOC-64).
#
# The reference implementation of the `figma` plugin kind (the eighth kind, and
# the last of the original roadmap). Dispatched by the `bwoc figma` CLI
# (BWOC-63), which owns argument parsing, the auth gate, and config resolution.
# This entry owns the HTTP: it reads Figma (node metadata, design tokens) and
# writes ONLY locally (exported images into a content-addressable cache under
# the workspace). It NEVER writes back to Figma — the kind is read-mostly by
# design. Contract: docs/en/PLUGINS.en.md §"Figma Asset Mapping Schema"
# (BWOC-62) + notes/2026-05-28_figma-plugin-architecture.md (BWOC-61).
#
# ── Invocation contract ────────────────────────────────────────────────────
# The CLI spawns this script with:
#   stdin                   one-line JSON request, e.g.
#                           {"operation":"fetch","file_key":"AbC123","node_ids":["1:2"]}
#   BWOC_FIGMA_OPERATION    the operation name (fetch | export | tokens) — fallback for stdin .operation
#   BWOC_WORKSPACE          absolute workspace root (export cache lives under it)
#   BWOC_PLUGIN_DIR         absolute path to this plugin's directory (informational)
#   BWOC_FIGMA_TOKEN        the Figma personal access token — SECRET (inherited env)
#
# On success: exit 0 and a single JSON object on stdout (the CLI parses it).
# On error:   a human message on stderr + non-zero exit (the CLI surfaces it).
#
# ── Security (Sila — Adinnaadana) ──────────────────────────────────────────
# The token is read from the environment (or a gitignored, owner-only
# .bwoc/secrets.toml) and handed to curl's X-Figma-Token header only. It is
# never echoed, never written to a file, and never placed in any JSON output or
# Asset Mapping entry. auth.toml in this directory ships the auth SHAPE with an
# EMPTY placeholder only.

set -euo pipefail

PLUGIN="figma-rest"
API_BASE="https://api.figma.com"
MAX_ATTEMPTS=4          # 429 backoff: total tries before giving up
DEFAULT_EXPORT_DIR="figma/exports"

# Globals set by _curl_figma.
HTTP_STATUS=0
HTTP_BODY=""

# ── stdin + dependencies ───────────────────────────────────────────────────

REQUEST="$(cat || true)"

for cmd in jq curl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN: required command '$cmd' not found on PATH — install it, then retry." >&2
    exit 1
  fi
done

emit_error_json() { # code, message — structured twin of the stderr diagnostic
  jq -n --arg code "$1" --arg msg "$2" \
    '{ok:false, plugin:"figma-rest", error:$code, message:$msg}'
}

req() { printf '%s' "$REQUEST" | jq -r "$1" 2>/dev/null || true; }

# Operation: prefer the stdin request, fall back to the env var.
OPERATION=""
if [[ -n "$REQUEST" ]]; then OPERATION="$(req '.operation // empty')"; fi
if [[ -z "$OPERATION" ]]; then OPERATION="${BWOC_FIGMA_OPERATION:-}"; fi
if [[ -z "$OPERATION" ]]; then
  printf '%s\n' "$PLUGIN: no operation (set BWOC_FIGMA_OPERATION or pipe a JSON request carrying .operation)" >&2
  exit 2
fi

WORKSPACE="${BWOC_WORKSPACE:-.}"
WORKSPACE="${WORKSPACE%/}"

# ── auth (env first; gitignored owner-only secrets file as fallback) ────────
#
# The token value is never logged. The env path touches no disk; the secrets
# fallback exists for hand-invocation and refuses a group/world-readable file.

TOKEN="${BWOC_FIGMA_TOKEN:-}"

_read_secret_token() { # echo the [figma].token from a chmod-600 secrets file, or nothing
  local f="$WORKSPACE/.bwoc/secrets.toml"
  [[ -f "$f" ]] || return 0
  local perms
  perms="$(stat -f '%Lp' "$f" 2>/dev/null || stat -c '%a' "$f" 2>/dev/null || echo '')"
  if [[ -n "$perms" && "${perms: -2}" != "00" ]]; then
    printf '%s\n' "$PLUGIN: refusing to read $f — it is group/world-readable (mode $perms); run 'chmod 600 $f'." >&2
    return 0
  fi
  awk '
    /^[[:space:]]*\[/ { insec = ($0 ~ /^[[:space:]]*\[figma\][[:space:]]*$/); next }
    insec && /^[[:space:]]*token[[:space:]]*=/ {
      line = $0
      sub(/^[^=]*=[[:space:]]*/, "", line)
      gsub(/^["'"'"']|["'"'"'][[:space:]]*$/, "", line)
      print line; exit
    }' "$f"
}

if [[ -z "$TOKEN" ]]; then TOKEN="$(_read_secret_token)"; fi

require_auth() {
  if [[ -z "$TOKEN" ]]; then
    printf '%s\n' "$PLUGIN: missing Figma token — set BWOC_FIGMA_TOKEN (or [figma].token in a chmod-600 .bwoc/secrets.toml). Never commit the token." >&2
    emit_error_json "auth_missing" "missing Figma token (set BWOC_FIGMA_TOKEN)"
    exit 2
  fi
}

# ── HTTP with retry/backoff + error-class mapping ──────────────────────────
#
# Temp files for header dump + body so we can read Retry-After on 429.

TMPDIR_PLUGIN="$(mktemp -d "${TMPDIR:-/tmp}/figma-rest.XXXXXX")"
HDR_FILE="$TMPDIR_PLUGIN/headers"
BODY_FILE="$TMPDIR_PLUGIN/body"
ERR_FILE="$TMPDIR_PLUGIN/curlerr"
trap 'rm -rf "$TMPDIR_PLUGIN"' EXIT

# _curl_figma <curl args...> — the X-Figma-Token header + JSON Accept are
# prepended. Honors HTTP 429 Retry-After (squared fallback) up to MAX_ATTEMPTS.
# Sets HTTP_STATUS (0 on transport failure) + HTTP_BODY.
_curl_figma() {
  local attempt=0 code ra
  while :; do
    attempt=$((attempt + 1))
    : >"$HDR_FILE"; : >"$BODY_FILE"; : >"$ERR_FILE"
    code="$(curl -sS \
      -H "X-Figma-Token: $TOKEN" \
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
      [[ "$ra" =~ ^[0-9]+$ ]] || ra=$((attempt * attempt)) # fallback when header absent
      sleep "$ra"
      continue
    fi
    return 0
  done
}

# classify_status <verb> [resource] — return 0 on 2xx; otherwise emit a clear
# diagnostic and exit. The 403 message names the scope gap the BWOC-61 note §2
# calls out (a file-scoped PAT cannot read a team library, and vice-versa).
classify_status() {
  local verb="$1" resource="${2:-resource}"
  case "$HTTP_STATUS" in
    2*) return 0 ;;
    0)
      printf '%s\n' "$PLUGIN $verb: network/transport error reaching Figma: $(printf '%s' "$HTTP_BODY" | head -c 300)" >&2
      exit 6 ;;
    401)
      printf '%s\n' "$PLUGIN $verb: authentication failed (HTTP 401) — the token is missing/expired/revoked. Rotate BWOC_FIGMA_TOKEN." >&2
      exit 3 ;;
    403)
      printf '%s\n' "$PLUGIN $verb: token lacks the required scope for $resource (HTTP 403) — a Figma PAT scoped to your own files cannot read a team library, and vice-versa. Check the token's scopes (see auth.toml [figma.auth.scopes])." >&2
      exit 3 ;;
    404)
      printf '%s\n' "$PLUGIN $verb: $resource not found (HTTP 404) — check the file_key / node_id." >&2
      exit 5 ;;
    429)
      printf '%s\n' "$PLUGIN $verb: rate limited (HTTP 429) after $MAX_ATTEMPTS attempts — retryable; back off and retry later." >&2
      exit 4 ;;
    *)
      printf '%s\n' "$PLUGIN $verb: Figma returned HTTP $HTTP_STATUS: $(printf '%s' "$HTTP_BODY" | head -c 300)" >&2
      exit 6 ;;
  esac
}

# resolve the comma-joined node id list from .node_ids[] (or a single .node_id).
node_ids_csv() {
  local csv
  csv="$(req '(.node_ids // (if .node_id then [.node_id] else [] end)) | join(",")')"
  printf '%s' "$csv"
}

_sha256() { # read stdin, print lowercase hex digest (portable: Linux + macOS)
  if command -v sha256sum >/dev/null 2>&1; then sha256sum | awk '{print $1}';
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 | awk '{print $1}';
  else return 1; fi
}

# jq program shared by `fetch` and `tokens`: projects /v1/files/:key/nodes into
# Asset Mapping entries. $with_tokens=true also extracts design tokens from each
# node's style properties (fills→colors, cornerRadius, text style, spacing).
NODES_TO_ASSETS='
  def clampbyte: (if . < 0 then 0 elif . > 255 then 255 else . end);
  def hexbyte: (.*255) | round | clampbyte | . as $n
    | ("0123456789abcdef"[($n/16|floor):($n/16|floor)+1]) + ("0123456789abcdef"[($n%16):($n%16)+1]);
  def colorhex: "#" + (.r|hexbyte) + (.g|hexbyte) + (.b|hexbyte);
  def tokens_of($d):
    ( ([ ($d.fills // [])[] | select(.type=="SOLID" and (.visible != false)) | .color ]
       | to_entries | map({ key: "color/fill/\(.key)", value: (.value | colorhex) }) | from_entries)
    + (if ($d.cornerRadius // null) != null then { "radius/corner": "\($d.cornerRadius)px" } else {} end)
    + (if (($d.strokes // []) | length) > 0 and ($d.strokeWeight // null) != null
         then { "border/width": "\($d.strokeWeight)px" } else {} end)
    + (if ($d.itemSpacing // null) != null then { "spacing/item": "\($d.itemSpacing)px" } else {} end)
    + (if ($d.style // null) != null then
         ( (if ($d.style.fontFamily // null)  != null then {"type/font-family": $d.style.fontFamily} else {} end)
         + (if ($d.style.fontSize // null)    != null then {"type/font-size": "\($d.style.fontSize)px"} else {} end)
         + (if ($d.style.fontWeight // null)  != null then {"type/font-weight": ($d.style.fontWeight|tostring)} else {} end)
         + (if ($d.style.lineHeightPx // null)!= null then {"type/line-height": "\($d.style.lineHeightPx|round)px"} else {} end)
         + (if ($d.style.letterSpacing // null)!= null then {"type/letter-spacing": "\($d.style.letterSpacing)px"} else {} end) )
       else {} end);
  . as $root
  | ($root.lastModified // null) as $lm
  | [ ($root.nodes // {}) | to_entries[]
      | .key as $nid | (.value.document // {}) as $doc
      | ( { file_key: $file_key, node_id: $nid, name: ($doc.name // $nid),
            type: ($doc.type // "UNKNOWN"), last_modified: $lm }
          + (if $with_tokens then (tokens_of($doc) as $t | if ($t|length) > 0 then {design_tokens: $t} else {} end) else {} end) ) ]
'

# ── Verb: fetch — node metadata → Asset Mapping entries ────────────────────

do_fetch() {
  require_auth
  local file_key ids
  file_key="$(req '.file_key // empty')"
  ids="$(node_ids_csv)"
  if [[ -z "$file_key" || -z "$ids" ]]; then
    printf '%s\n' "$PLUGIN fetch: both .file_key and .node_ids (or .node_id) are required" >&2
    emit_error_json "bad_args" "file_key and node_ids required"
    exit 2
  fi

  _curl_figma -G "${API_BASE}/v1/files/${file_key}/nodes" --data-urlencode "ids=${ids}"
  classify_status "fetch" "file '${file_key}'"

  printf '%s' "$HTTP_BODY" | jq \
    --arg file_key "$file_key" --argjson with_tokens false \
    "{ ok: true, operation: \"fetch\", file_key: \$file_key, assets: ($NODES_TO_ASSETS) }"
}

# ── Verb: tokens — node styles → Asset Mapping entries w/ design_tokens ────

do_tokens() {
  require_auth
  local file_key ids
  file_key="$(req '.file_key // empty')"
  ids="$(node_ids_csv)"
  if [[ -z "$file_key" || -z "$ids" ]]; then
    printf '%s\n' "$PLUGIN tokens: both .file_key and .node_ids (or .node_id) are required" >&2
    emit_error_json "bad_args" "file_key and node_ids required"
    exit 2
  fi

  _curl_figma -G "${API_BASE}/v1/files/${file_key}/nodes" --data-urlencode "ids=${ids}"
  classify_status "tokens" "file '${file_key}'"

  printf '%s' "$HTTP_BODY" | jq \
    --arg file_key "$file_key" --argjson with_tokens true \
    "{ ok: true, operation: \"tokens\", file_key: \$file_key, assets: ($NODES_TO_ASSETS) }"
}

# ── Verb: export — render a node + cache it content-addressably ────────────
#
# The cache key is SHA-256(file_key + node_id + version + format): an unchanged
# node (same file version) re-exports to the same filename, so a cache hit
# skips the heavy, rate-limited image-render + download. When the caller already
# holds the file version + node metadata (from a prior `fetch`), pass them in to
# make a hit a zero-API operation; otherwise one cheap metadata read resolves
# them and populates the Asset Mapping entry.

do_export() {
  require_auth
  local file_key node_id format scale version name type last_modified export_dir
  file_key="$(req '.file_key // empty')"
  node_id="$(req '.node_id // empty')"
  format="$(req '.format // "png"')"
  scale="$(req '.scale // 1')"
  version="$(req '.version // empty')"
  name="$(req '.name // empty')"
  type="$(req '.type // empty')"
  last_modified="$(req '.last_modified // empty')"
  export_dir="$(req ".export_dir // \"$DEFAULT_EXPORT_DIR\"")"

  if [[ -z "$file_key" || -z "$node_id" ]]; then
    printf '%s\n' "$PLUGIN export: both .file_key and .node_id are required" >&2
    emit_error_json "bad_args" "file_key and node_id required"
    exit 2
  fi
  case "$format" in png|jpg|svg|pdf) ;; *)
    printf '%s\n' "$PLUGIN export: unsupported format '$format' (expected png | jpg | svg | pdf)" >&2
    emit_error_json "bad_args" "unsupported format '$format'"
    exit 2 ;;
  esac
  [[ "$scale" =~ ^[0-9]+([.][0-9]+)?$ ]] || scale=1

  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN export: no SHA-256 tool found (need sha256sum or shasum) — required for the content-addressable cache key." >&2
    emit_error_json "missing_dep" "no sha256sum/shasum for cache key"
    exit 1
  fi

  # Resolve the file version (+ metadata for the entry) unless the caller passed
  # everything needed. The version is the cache-invalidation signal.
  if [[ -z "$version" || -z "$name" || -z "$type" || -z "$last_modified" ]]; then
    _curl_figma -G "${API_BASE}/v1/files/${file_key}/nodes" --data-urlencode "ids=${node_id}"
    classify_status "export" "file '${file_key}'"
    version="$(printf '%s' "$HTTP_BODY" | jq -r '.version // empty')"
    last_modified="$(printf '%s' "$HTTP_BODY" | jq -r '.lastModified // empty')"
    name="$(printf '%s' "$HTTP_BODY" | jq -r --arg n "$node_id" '.nodes[$n].document.name // empty')"
    type="$(printf '%s' "$HTTP_BODY" | jq -r --arg n "$node_id" '.nodes[$n].document.type // empty')"
    if [[ -z "$version" ]]; then
      printf '%s\n' "$PLUGIN export: could not resolve file version for '$file_key' (node '$node_id' may not exist)." >&2
      emit_error_json "not_found" "node '$node_id' not found in file '$file_key'"
      exit 5
    fi
  fi

  local sha rel abs
  sha="$(printf '%s' "${file_key}${node_id}${version}${format}" | _sha256)"
  rel="${export_dir%/}/${sha}.${format}"
  abs="${WORKSPACE}/${rel}"

  # Cache hit — the rendered artifact for this (file, node, version, format)
  # already exists. No render, no download, no API call beyond the version read.
  if [[ -f "$abs" ]]; then
    jq -n --arg fk "$file_key" --arg nid "$node_id" --arg name "$name" \
      --arg type "$type" --arg lm "$last_modified" --arg path "$rel" \
      '{ ok:true, operation:"export", cached:true,
         asset: ({ file_key:$fk, node_id:$nid, name:$name, type:$type, last_modified:$lm, exported_path:$path }
                 | with_entries(select(.value != ""))) }'
    return 0
  fi

  # Cache miss — request the render URL, then download it. The render URL is a
  # short-lived, pre-signed Figma-hosted URL (no auth header on the download).
  _curl_figma -G "${API_BASE}/v1/images/${file_key}" \
    --data-urlencode "ids=${node_id}" \
    --data-urlencode "format=${format}" \
    --data-urlencode "scale=${scale}"
  classify_status "export" "image for node '${node_id}'"

  local image_url err
  err="$(printf '%s' "$HTTP_BODY" | jq -r '.err // empty')"
  image_url="$(printf '%s' "$HTTP_BODY" | jq -r --arg n "$node_id" '.images[$n] // empty')"
  if [[ -n "$err" || -z "$image_url" ]]; then
    printf '%s\n' "$PLUGIN export: Figma could not render node '$node_id'${err:+ ($err)} — it may be un-exportable for format '$format'." >&2
    emit_error_json "render_failed" "could not render node '$node_id'${err:+ ($err)}"
    exit 7
  fi

  mkdir -p "$(dirname "$abs")"
  if ! curl -sS -fL -o "$abs" "$image_url" 2>"$ERR_FILE"; then
    rm -f "$abs"
    printf '%s\n' "$PLUGIN export: failed to download rendered image: $(cat "$ERR_FILE" 2>/dev/null | head -c 200)" >&2
    emit_error_json "download_failed" "failed to download rendered image for node '$node_id'"
    exit 6
  fi

  jq -n --arg fk "$file_key" --arg nid "$node_id" --arg name "$name" \
    --arg type "$type" --arg lm "$last_modified" --arg path "$rel" --arg url "$image_url" \
    '{ ok:true, operation:"export", cached:false,
       asset: ({ file_key:$fk, node_id:$nid, name:$name, type:$type, last_modified:$lm,
                 exported_path:$path, image_url:$url }
               | with_entries(select(.value != ""))) }'
}

# ── Dispatch ───────────────────────────────────────────────────────────────

case "$OPERATION" in
  fetch)  do_fetch ;;
  export) do_export ;;
  tokens) do_tokens ;;
  *)
    printf '%s\n' "$PLUGIN: unknown operation '$OPERATION' (expected fetch | export | tokens)" >&2
    emit_error_json "unknown_operation" "unknown operation '$OPERATION'"
    exit 2 ;;
esac
