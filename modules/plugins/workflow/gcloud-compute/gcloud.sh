#!/usr/bin/env bash
#
# gcloud-compute — workflow/gcloud-compute plugin entry (EPIC-9, BWOC-EPIC-9).
#
# The first write-capable GCP slice. Verbs:
#   read : list | describe
#   write: start | stop   (gated in the `bwoc gcloud compute` CLI, not here)
#
# Sources credential helpers from the sibling workflow/gcloud-auth plugin —
# auth resolution lives there exactly once (EPIC-8 design note §Decision 2).
# Sourcing is BASH_SOURCE-guarded on the sibling side, so importing the helpers
# does not run the gcloud-auth dispatcher.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"list","zone":"us-central1-a"}
#                          {"operation":"describe","instance":"web-1","zone":"us-central1-a"}
#                          {"operation":"stop","instance":"web-1","zone":"us-central1-a"}
#   BWOC_GCLOUD_OPERATION  fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (resolves the SA JSON path)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#                          (used to find ../gcloud-auth/gcloud.sh)
#
# Security (Sila — Adinnaadana + option-injection guard, #92):
#   The plugin reads NO credential value. Every operator-supplied value reaches
#   `gcloud` either as a `--flag=value` (the value is bound, never re-parsed) or
#   as a positional AFTER a `--` end-of-options separator, so a `-`-leading
#   instance id can never be read as a gcloud flag. auth.toml ships SHAPE only.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gcloud_compute_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gcloud_compute_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gcloud-auth/gcloud.sh")
  fi
  candidates+=("$(_gcloud_compute_self_dir)/../gcloud-auth/gcloud.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gcloud_compute_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gcloud-compute: sibling helpers workflow/gcloud-auth/gcloud.sh not found — install workflow/gcloud-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

PLUGIN="gcloud-compute"

# ── helpers ─────────────────────────────────────────────────────────────────

# _gcloud_compute_field — extract a string field from the request JSON, empty
# when absent. Centralizes the `jq -r` so each verb stays readable.
_gcloud_compute_field() {
  printf '%s' "$1" | jq -r --arg k "$2" '.[$k] // empty' 2>/dev/null || true
}

# Echo `--project=<id>` when the request carries a non-empty project, else
# nothing. `=`-bound so the value is never re-parsed as an option.
_gcloud_compute_project_flag() {
  local p="$1"
  if [[ -n "$p" ]]; then printf -- '--project=%s' "$p"; fi
}

# ── verbs ─────────────────────────────────────────────────────────────────

_gcloud_compute_list() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local zone project
  zone="$(_gcloud_compute_field "$request" zone)"
  project="$(_gcloud_compute_field "$request" project)"

  local -a args=(compute instances list --format=json)
  [[ -n "$zone" ]] && args+=("--zones=${zone}")
  local pflag; pflag="$(_gcloud_compute_project_flag "$project")"
  [[ -n "$pflag" ]] && args+=("$pflag")

  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN list: 'gcloud compute instances list' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq '{
    ok: true,
    plugin: "gcloud-compute",
    operation: "list",
    total: (length),
    instances: [ .[] | {
      name:         (.name // null),
      zone:         ((.zone // "") | sub(".*/"; "")),
      status:       (.status // null),
      machine_type: ((.machineType // "") | sub(".*/"; "")),
      internal_ip:  (.networkInterfaces[0].networkIP // null)
    } ]
  }'
}

_gcloud_compute_describe() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local instance zone project
  instance="$(_gcloud_compute_field "$request" instance)"
  zone="$(_gcloud_compute_field "$request" zone)"
  project="$(_gcloud_compute_field "$request" project)"
  if [[ -z "$instance" || -z "$zone" ]]; then
    printf '%s\n' "$PLUGIN describe: .instance and .zone are required" >&2
    exit 2
  fi

  local -a args=(compute instances describe "--zone=${zone}" --format=json)
  local pflag; pflag="$(_gcloud_compute_project_flag "$project")"
  [[ -n "$pflag" ]] && args+=("$pflag")
  args+=(-- "$instance")

  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN describe: 'gcloud compute instances describe $instance' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq '{
    ok: true,
    plugin: "gcloud-compute",
    operation: "describe",
    name:         (.name // null),
    zone:         ((.zone // "") | sub(".*/"; "")),
    status:       (.status // null),
    machine_type: ((.machineType // "") | sub(".*/"; "")),
    internal_ip:  (.networkInterfaces[0].networkIP // null),
    external_ip:  (.networkInterfaces[0].accessConfigs[0].natIP // null),
    create_time:  (.creationTimestamp // null)
  }'
}

# Shared start/stop body — `verb` is "start" or "stop". gcloud waits for the
# operation, then we re-describe to report the *actual* resulting status
# (Sacca — report what is true, not the intended state).
_gcloud_compute_lifecycle() {
  local request="$1" verb="$2"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local instance zone project
  instance="$(_gcloud_compute_field "$request" instance)"
  zone="$(_gcloud_compute_field "$request" zone)"
  project="$(_gcloud_compute_field "$request" project)"
  if [[ -z "$instance" || -z "$zone" ]]; then
    printf '%s\n' "$PLUGIN $verb: .instance and .zone are required" >&2
    exit 2
  fi

  local pflag; pflag="$(_gcloud_compute_project_flag "$project")"
  local -a args=(compute instances "$verb" "--zone=${zone}" --quiet)
  [[ -n "$pflag" ]] && args+=("$pflag")
  args+=(-- "$instance")

  local err
  if ! err="$(gcloud "${args[@]}" 2>&1 >/dev/null)"; then
    printf '%s\n' "$PLUGIN $verb: 'gcloud compute instances $verb $instance' failed: $(printf '%s' "$err" | head -c 300)" >&2
    exit 6
  fi

  # Re-read the actual status after the (synchronous) lifecycle op.
  local -a dargs=(compute instances describe "--zone=${zone}" --format='value(status)')
  [[ -n "$pflag" ]] && dargs+=("$pflag")
  dargs+=(-- "$instance")
  local status
  status="$(gcloud "${dargs[@]}" 2>/dev/null || true)"

  jq -n \
    --arg verb "$verb" \
    --arg instance "$instance" \
    --arg zone "$zone" \
    --arg status "$status" \
    '{
      ok: true,
      plugin: "gcloud-compute",
      operation: $verb,
      instance: $instance,
      zone: $zone,
      status: (if $status == "" then null else $status end)
    }'
}

# ── dispatch ───────────────────────────────────────────────────────────────

_gcloud_compute_main() {
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
    list)     _gcloud_compute_list "$request" ;;
    describe) _gcloud_compute_describe "$request" ;;
    start)    _gcloud_compute_lifecycle "$request" start ;;
    stop)     _gcloud_compute_lifecycle "$request" stop ;;
    *)
      printf '%s\n' "$PLUGIN: unknown operation '$operation' (expected list | describe | start | stop)" >&2
      exit 2 ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _gcloud_compute_main
fi
