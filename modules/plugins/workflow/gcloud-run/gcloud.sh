#!/usr/bin/env bash
#
# gcloud-run — workflow/gcloud-run plugin entry (EPIC-11).
#
# The third write-capable GCP slice (Cloud Run). Verbs:
#   read : list | describe
#   write: deploy   (gated in the `bwoc gcloud run` CLI — T2: confirm + echoed
#                    service/region/source/traffic; not gated here)
#
# Sources credential helpers from the sibling workflow/gcloud-auth plugin
# (EPIC-8 design note §Decision 2). Sourcing is BASH_SOURCE-guarded on the
# sibling side, so importing the helpers does not run its dispatcher.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"list","region":"us-central1"}
#                          {"operation":"describe","service":"api","region":"us-central1"}
#                          {"operation":"deploy","service":"api","region":"us-central1","image":"gcr.io/p/api:v2"}
#                          {"operation":"deploy","service":"api","region":"us-central1","source":"/abs/src"}
#   BWOC_GCLOUD_OPERATION  fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (resolves the SA JSON path)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#
# Security (Sila — Adinnaadana + option-injection guard, #92):
#   No credential value is read. Every operator value reaches `gcloud` as a
#   `--flag=value` (bound) or as a positional AFTER a `--` separator (the
#   service name), so a `-`-leading value can never be parsed as a gcloud flag.
#   `--source` is an absolute path the CLI canonicalized before dispatch.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gcloud_run_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gcloud_run_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gcloud-auth/gcloud.sh")
  fi
  candidates+=("$(_gcloud_run_self_dir)/../gcloud-auth/gcloud.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gcloud_run_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gcloud-run: sibling helpers workflow/gcloud-auth/gcloud.sh not found — install workflow/gcloud-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

PLUGIN="gcloud-run"

# ── helpers ─────────────────────────────────────────────────────────────────

_gcloud_run_field() {
  printf '%s' "$1" | jq -r --arg k "$2" '.[$k] // empty' 2>/dev/null || true
}

_gcloud_run_project_flag() {
  local p="$1"
  if [[ -n "$p" ]]; then printf -- '--project=%s' "$p"; fi
}

# ── verbs ─────────────────────────────────────────────────────────────────

_gcloud_run_list() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local region project
  region="$(_gcloud_run_field "$request" region)"
  project="$(_gcloud_run_field "$request" project)"

  local -a args=(run services list --format=json)
  [[ -n "$region" ]] && args+=("--region=${region}")
  local pflag; pflag="$(_gcloud_run_project_flag "$project")"
  [[ -n "$pflag" ]] && args+=("$pflag")

  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN list: 'gcloud run services list' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq '{
    ok: true,
    plugin: "gcloud-run",
    operation: "list",
    total: (length),
    services: [ .[] | {
      name:   (.metadata.name // .name // null),
      region: (.metadata.labels["cloud.googleapis.com/location"] // .region // null),
      url:    (.status.url // .url // null),
      ready:  ((.status.conditions // []) | map(select(.type=="Ready")) | (.[0].status // null))
    } ]
  }'
}

_gcloud_run_describe() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local service region project
  service="$(_gcloud_run_field "$request" service)"
  region="$(_gcloud_run_field "$request" region)"
  project="$(_gcloud_run_field "$request" project)"
  if [[ -z "$service" || -z "$region" ]]; then
    printf '%s\n' "$PLUGIN describe: .service and .region are required" >&2
    exit 2
  fi

  local -a args=(run services describe "--region=${region}" --format=json)
  local pflag; pflag="$(_gcloud_run_project_flag "$project")"
  [[ -n "$pflag" ]] && args+=("$pflag")
  args+=(-- "$service")

  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN describe: 'gcloud run services describe $service' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq --arg s "$service" --arg r "$region" '{
    ok: true,
    plugin: "gcloud-run",
    operation: "describe",
    service: (.metadata.name // $s),
    region: $r,
    url: (.status.url // null),
    latest_ready_revision: (.status.latestReadyRevisionName // null),
    latest_created_revision: (.status.latestCreatedRevisionName // null),
    traffic: [ (.status.traffic // [])[] | { revision: (.revisionName // null), percent: (.percent // null), latest: (.latestRevision // false) } ]
  }'
}

_gcloud_run_deploy() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local service region project image source
  service="$(_gcloud_run_field "$request" service)"
  region="$(_gcloud_run_field "$request" region)"
  project="$(_gcloud_run_field "$request" project)"
  image="$(_gcloud_run_field "$request" image)"
  source="$(_gcloud_run_field "$request" source)"
  if [[ -z "$service" || -z "$region" ]]; then
    printf '%s\n' "$PLUGIN deploy: .service and .region are required" >&2
    exit 2
  fi
  if [[ -z "$image" && -z "$source" ]]; then
    printf '%s\n' "$PLUGIN deploy: exactly one of .image or .source is required" >&2
    exit 2
  fi
  if [[ -n "$image" && -n "$source" ]]; then
    printf '%s\n' "$PLUGIN deploy: .image and .source are mutually exclusive" >&2
    exit 2
  fi

  # --quiet: the BWOC CLI owns the confirmation gate (T2); suppress gcloud's own.
  local -a args=(run deploy "--region=${region}" --format=json --quiet)
  local pflag; pflag="$(_gcloud_run_project_flag "$project")"
  [[ -n "$pflag" ]] && args+=("$pflag")
  [[ -n "$image" ]] && args+=("--image=${image}")
  [[ -n "$source" ]] && args+=("--source=${source}")
  args+=(-- "$service")

  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN deploy: 'gcloud run deploy $service' failed: $(printf '%s' "$raw" | head -c 400)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq --arg s "$service" --arg r "$region" '{
    ok: true,
    plugin: "gcloud-run",
    operation: "deploy",
    service: (.metadata.name // $s),
    region: $r,
    url: (.status.url // null),
    latest_ready_revision: (.status.latestReadyRevisionName // null)
  }'
}

# ── dispatch ───────────────────────────────────────────────────────────────

_gcloud_run_main() {
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
    list)     _gcloud_run_list "$request" ;;
    describe) _gcloud_run_describe "$request" ;;
    deploy)   _gcloud_run_deploy "$request" ;;
    *)
      printf '%s\n' "$PLUGIN: unknown operation '$operation' (expected list | describe | deploy)" >&2
      exit 2 ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _gcloud_run_main
fi
