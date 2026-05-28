#!/usr/bin/env bash
#
# gcloud-project — workflow/gcloud-project plugin entry (BWOC-53).
#
# Verbs: list | show | set-default.
#
# Sources credential helpers from the sibling workflow/gcloud-auth plugin —
# the foundation auth resolution lives there exactly once (decision 2 of the
# BWOC-51 design note). Sourcing is BASH_SOURCE-guarded on the sibling side,
# so importing the helpers does not run the gcloud-auth dispatcher.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"list"}
#                          {"operation":"show","project":"my-proj"}
#                          {"operation":"set-default","project":"my-proj"}
#   BWOC_GCLOUD_OPERATION  fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (resolves the SA JSON path)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#                          (used to find ../gcloud-auth/gcloud.sh)
#
# Security (Sila — Adinnaadana):
#   This plugin never reads any credential value. It only asks the local
#   `gcloud` CLI for state. auth.toml ships SHAPE only — no values.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
# Prefer the explicit BWOC_PLUGIN_DIR (set by the framework dispatcher); fall
# back to a script-relative path so the plugin remains testable without the
# dispatcher. The source path is hardcoded relative to the workspace plugin
# tree — no PATH games (design note §Decision 2).
_gcloud_project_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gcloud_project_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gcloud-auth/gcloud.sh")
  fi
  candidates+=("$(_gcloud_project_self_dir)/../gcloud-auth/gcloud.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gcloud_project_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gcloud-project: sibling helpers workflow/gcloud-auth/gcloud.sh not found — install workflow/gcloud-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

# The sourced helpers set PLUGIN="gcloud-auth"; override AFTER sourcing so
# this plugin's diagnostics name itself. The helper functions still print
# under the current $PLUGIN value, which becomes "gcloud-project" — accurate
# for the caller.
PLUGIN="gcloud-project"

# ── verbs ──────────────────────────────────────────────────────────────────

_gcloud_project_list() {
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local raw
  if ! raw="$(gcloud projects list --format=json 2>&1)"; then
    printf '%s\n' "$PLUGIN list: 'gcloud projects list' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi

  printf '%s' "$raw" | jq '{
    ok: true,
    plugin: "gcloud-project",
    operation: "list",
    total: (length),
    projects: [ .[] | {
      project_id:      (.projectId // null),
      project_number:  (.projectNumber // null),
      name:            (.name // null),
      lifecycle_state: (.lifecycleState // null)
    } ]
  }'
}

_gcloud_project_show() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local project
  project="$(printf '%s' "$request" | jq -r '.project // empty' 2>/dev/null || true)"
  if [[ -z "$project" ]]; then
    project="$(gcloud config get-value project 2>/dev/null || true)"
  fi
  if [[ -z "$project" || "$project" == "(unset)" ]]; then
    printf '%s\n' "$PLUGIN show: no project (pass {.project} or 'gcloud config set project <id>')" >&2
    exit 2
  fi

  # `--` ends option parsing so a `-`-leading project id is a positional, not a
  # gcloud flag (option-injection guard, #91).
  local raw
  if ! raw="$(gcloud projects describe --format=json -- "$project" 2>&1)"; then
    printf '%s\n' "$PLUGIN show: 'gcloud projects describe $project' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi

  printf '%s' "$raw" | jq --arg p "$project" '{
    ok: true,
    plugin: "gcloud-project",
    operation: "show",
    project_id:      (.projectId // $p),
    project_number:  (.projectNumber // null),
    name:            (.name // null),
    lifecycle_state: (.lifecycleState // null),
    create_time:     (.createTime // null),
    parent:          (.parent // null),
    labels:          (.labels // {})
  }'
}

_gcloud_project_set_default() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local project
  project="$(printf '%s' "$request" | jq -r '.project // empty' 2>/dev/null || true)"
  if [[ -z "$project" ]]; then
    printf '%s\n' "$PLUGIN set-default: .project required (pass {\"project\":\"<id>\"})" >&2
    exit 2
  fi

  # Capture the previous default so the response is reversible-by-inspection.
  local previous
  previous="$(gcloud config get-value project 2>/dev/null || true)"
  if [[ "$previous" == "(unset)" ]]; then previous=""; fi

  # `gcloud config set` mutates ~/.config/gcloud/configurations/... only — no
  # remote API call. The CLI gates this verb behind operator confirmation
  # (BWOC-52); the plugin itself trusts the caller. Reversibility is trivial
  # (`gcloud config set project <previous>`).
  # `--` ends option parsing so a `-`-leading project id is the value, not a
  # gcloud flag (option-injection guard, #91).
  if ! gcloud config set project -- "$project" >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN set-default: 'gcloud config set project $project' failed" >&2
    exit 6
  fi

  jq -n --arg prev "$previous" --arg curr "$project" '{
    ok: true,
    plugin: "gcloud-project",
    operation: "set-default",
    previous: (if $prev == "" then null else $prev end),
    current: $curr,
    note: "Local gcloud config only; no remote API mutation."
  }'
}

# ── dispatch ───────────────────────────────────────────────────────────────

_gcloud_project_main() {
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
    list)         _gcloud_project_list ;;
    show)         _gcloud_project_show "$request" ;;
    set-default)  _gcloud_project_set_default "$request" ;;
    *)
      printf '%s\n' "$PLUGIN: unknown operation '$operation' (expected list | show | set-default)" >&2
      exit 2 ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _gcloud_project_main
fi
