#!/usr/bin/env bash
#
# gcloud-storage — workflow/gcloud-storage plugin entry (EPIC-10).
#
# The second write-capable GCP slice — and the first with an IRREVERSIBLE
# write. Verbs:
#   read : list | stat
#   write: put  | delete   (gated in the `bwoc gcloud storage` CLI, not here;
#                           delete is T3 — typed-name confirmation)
#
# Sources credential helpers from the sibling workflow/gcloud-auth plugin
# (EPIC-8 design note §Decision 2). Sourcing is BASH_SOURCE-guarded on the
# sibling side, so importing the helpers does not run its dispatcher.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"list","bucket":"my-bkt","prefix":"logs/"}
#                          {"operation":"stat","bucket":"my-bkt","object":"a.txt"}
#                          {"operation":"put","bucket":"my-bkt","object":"a.txt","local":"./a.txt"}
#                          {"operation":"delete","bucket":"my-bkt","object":"a.txt"}
#   BWOC_GCLOUD_OPERATION  fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (resolves the SA JSON path)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#
# Security (Sila — Adinnaadana + option-injection guard, #92):
#   No credential value is read. Every operator value reaches `gcloud` as a
#   `--flag=value` (bound) or as a positional AFTER a `--` separator, so a
#   `-`-leading bucket/object/path can never be parsed as a gcloud flag.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gcloud_storage_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gcloud_storage_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gcloud-auth/gcloud.sh")
  fi
  candidates+=("$(_gcloud_storage_self_dir)/../gcloud-auth/gcloud.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gcloud_storage_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gcloud-storage: sibling helpers workflow/gcloud-auth/gcloud.sh not found — install workflow/gcloud-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

PLUGIN="gcloud-storage"

# ── helpers ─────────────────────────────────────────────────────────────────

_gcloud_storage_field() {
  printf '%s' "$1" | jq -r --arg k "$2" '.[$k] // empty' 2>/dev/null || true
}

# Echo `--project=<id>` for a non-empty project, else nothing (`=`-bound).
_gcloud_storage_project_flag() {
  local p="$1"
  if [[ -n "$p" ]]; then printf -- '--project=%s' "$p"; fi
}

_gcloud_storage_require_object() {
  local bucket="$1" object="$2" verb="$3"
  if [[ -z "$bucket" || -z "$object" ]]; then
    printf '%s\n' "$PLUGIN $verb: .bucket and .object are required" >&2
    exit 2
  fi
}

# ── verbs ─────────────────────────────────────────────────────────────────

_gcloud_storage_list() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local bucket prefix project
  bucket="$(_gcloud_storage_field "$request" bucket)"
  prefix="$(_gcloud_storage_field "$request" prefix)"
  project="$(_gcloud_storage_field "$request" project)"
  if [[ -z "$bucket" ]]; then
    printf '%s\n' "$PLUGIN list: .bucket is required" >&2
    exit 2
  fi

  local url="gs://${bucket}"
  [[ -n "$prefix" ]] && url="${url}/${prefix}"
  local -a args=(storage ls --format=json)
  local pflag; pflag="$(_gcloud_storage_project_flag "$project")"
  [[ -n "$pflag" ]] && args+=("$pflag")
  args+=(-- "$url")

  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN list: 'gcloud storage ls $url' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq --arg b "$bucket" '{
    ok: true,
    plugin: "gcloud-storage",
    operation: "list",
    bucket: $b,
    total: (length),
    objects: [ .[] | { url: (.url // .), size: (.size // null), updated: (.updated // null) } ]
  }'
}

_gcloud_storage_stat() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local bucket object project
  bucket="$(_gcloud_storage_field "$request" bucket)"
  object="$(_gcloud_storage_field "$request" object)"
  project="$(_gcloud_storage_field "$request" project)"
  _gcloud_storage_require_object "$bucket" "$object" stat

  local url="gs://${bucket}/${object}"
  local pflag; pflag="$(_gcloud_storage_project_flag "$project")"
  local -a dargs=(storage objects describe --format=json)
  [[ -n "$pflag" ]] && dargs+=("$pflag")
  dargs+=(-- "$url")
  local errf out
  errf="$(mktemp)"
  # `if cmd; then` exempts the describe from set -e so we can branch on a
  # clean not-found (→ exists:false) vs a real error (→ exit 6).
  if out="$(gcloud "${dargs[@]}" 2>"$errf")"; then
    rm -f "$errf"
    printf '%s' "$out" | jq --arg b "$bucket" --arg o "$object" '{
      ok: true,
      plugin: "gcloud-storage",
      operation: "stat",
      exists: true,
      bucket: $b,
      object: $o,
      size: (.size // null),
      updated: (.updated // null),
      content_type: (.content_type // .contentType // null),
      storage_class: (.storage_class // .storageClass // null)
    }'
  else
    local err; err="$(cat "$errf" 2>/dev/null || true)"; rm -f "$errf"
    if printf '%s' "$err" | grep -qiE "not found|does not exist|no url(s)? matched|no object|404"; then
      jq -n --arg b "$bucket" --arg o "$object" '{
        ok: true, plugin: "gcloud-storage", operation: "stat",
        exists: false, bucket: $b, object: $o
      }'
    else
      printf '%s\n' "$PLUGIN stat: 'gcloud storage objects describe $url' failed: $(printf '%s' "$err" | head -c 300)" >&2
      exit 6
    fi
  fi
}

_gcloud_storage_put() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local bucket object local_path project
  bucket="$(_gcloud_storage_field "$request" bucket)"
  object="$(_gcloud_storage_field "$request" object)"
  local_path="$(_gcloud_storage_field "$request" local)"
  project="$(_gcloud_storage_field "$request" project)"
  _gcloud_storage_require_object "$bucket" "$object" put
  if [[ -z "$local_path" ]]; then
    printf '%s\n' "$PLUGIN put: .local (source path) is required" >&2
    exit 2
  fi
  if [[ ! -f "$local_path" ]]; then
    printf '%s\n' "$PLUGIN put: local source '$local_path' not found" >&2
    exit 2
  fi

  local url="gs://${bucket}/${object}"
  local pflag; pflag="$(_gcloud_storage_project_flag "$project")"
  local -a args=(storage cp)
  [[ -n "$pflag" ]] && args+=("$pflag")
  args+=(-- "$local_path" "$url")

  local err
  if ! err="$(gcloud "${args[@]}" 2>&1 >/dev/null)"; then
    printf '%s\n' "$PLUGIN put: 'gcloud storage cp -> $url' failed: $(printf '%s' "$err" | head -c 300)" >&2
    exit 6
  fi
  jq -n --arg b "$bucket" --arg o "$object" --arg l "$local_path" '{
    ok: true, plugin: "gcloud-storage", operation: "put",
    bucket: $b, object: $o, source: $l
  }'
}

_gcloud_storage_delete() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local bucket object project
  bucket="$(_gcloud_storage_field "$request" bucket)"
  object="$(_gcloud_storage_field "$request" object)"
  project="$(_gcloud_storage_field "$request" project)"
  _gcloud_storage_require_object "$bucket" "$object" delete

  local url="gs://${bucket}/${object}"
  local pflag; pflag="$(_gcloud_storage_project_flag "$project")"
  local -a args=(storage rm)
  [[ -n "$pflag" ]] && args+=("$pflag")
  args+=(-- "$url")

  local err
  if ! err="$(gcloud "${args[@]}" 2>&1 >/dev/null)"; then
    printf '%s\n' "$PLUGIN delete: 'gcloud storage rm $url' failed: $(printf '%s' "$err" | head -c 300)" >&2
    exit 6
  fi
  jq -n --arg b "$bucket" --arg o "$object" '{
    ok: true, plugin: "gcloud-storage", operation: "delete",
    bucket: $b, object: $o, deleted: true
  }'
}

# ── dispatch ───────────────────────────────────────────────────────────────

_gcloud_storage_main() {
  if ! command -v jq >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN: required command 'jq' not found on PATH" >&2
    exit 1
  fi

  local request operation
  request="$(cat || true)"
  operation=""
  if [[ -n "$request" ]]; then
    operation="$(printf '%s' "$request" | jq -r '.operation // empty' 2>/dev/null || true)"
  fi
  if [[ -z "$operation" ]]; then operation="${BWOC_GCLOUD_OPERATION:-}"; fi
  if [[ -z "$operation" ]]; then
    printf '%s\n' "$PLUGIN: no operation (set BWOC_GCLOUD_OPERATION or pipe a JSON request carrying .operation)" >&2
    exit 2
  fi

  case "$operation" in
    list)   _gcloud_storage_list "$request" ;;
    stat)   _gcloud_storage_stat "$request" ;;
    put)    _gcloud_storage_put "$request" ;;
    delete) _gcloud_storage_delete "$request" ;;
    *)
      printf '%s\n' "$PLUGIN: unknown operation '$operation' (expected list | stat | put | delete)" >&2
      exit 2 ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _gcloud_storage_main
fi
