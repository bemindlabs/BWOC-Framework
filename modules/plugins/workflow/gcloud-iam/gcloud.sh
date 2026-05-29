#!/usr/bin/env bash
#
# gcloud-iam — workflow/gcloud-iam plugin entry (EPIC-12, LAST).
#
# The fourth and final write-capable GCP slice (project IAM policy). Verbs:
#   read : get      (gcloud projects get-iam-policy)
#   write: add      (gcloud projects add-iam-policy-binding)     — gated T4 in CLI
#   write: remove   (gcloud projects remove-iam-policy-binding)  — gated T4 in CLI
#
# Sources credential helpers from the sibling workflow/gcloud-auth plugin
# (EPIC-8 design note §Decision 2). Sourcing is BASH_SOURCE-guarded on the
# sibling side, so importing the helpers does not run its dispatcher.
#
# Contract:
#   stdin                  one-line JSON, e.g.
#                          {"operation":"get","project":"my-proj"}
#                          {"operation":"add","project":"my-proj","member":"user:x@y.com","role":"roles/viewer"}
#                          {"operation":"remove","project":"my-proj","member":"user:x@y.com","role":"roles/viewer"}
#   BWOC_GCLOUD_OPERATION  fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (resolves the SA JSON path)
#   BWOC_PLUGIN_DIR        absolute path to THIS plugin's directory
#
# Security (Sila — Adinnaadana + option-injection guard, #92):
#   No credential value is read. Every operator value reaches `gcloud` as a
#   `--flag=value` (member/role bound) or as a positional AFTER a `--` separator
#   (the project id), so a `-`-leading value can never be parsed as a gcloud
#   flag. ALL write gating (T4 standing opt-in + typed confirm, public-principal
#   refusal, high-privilege warning) lives in the `bwoc gcloud iam` CLI, never
#   here — this entry only executes the already-vetted request.

set -euo pipefail

# ── source sibling auth helpers ────────────────────────────────────────────
_gcloud_iam_self_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

_gcloud_iam_resolve_helpers() {
  local candidates=()
  if [[ -n "${BWOC_PLUGIN_DIR:-}" ]]; then
    candidates+=("${BWOC_PLUGIN_DIR%/}/../gcloud-auth/gcloud.sh")
  fi
  candidates+=("$(_gcloud_iam_self_dir)/../gcloud-auth/gcloud.sh")
  local c
  for c in "${candidates[@]}"; do
    if [[ -n "$c" && -r "$c" ]]; then printf '%s' "$c"; return 0; fi
  done
  return 1
}

_AUTH_HELPERS="$(_gcloud_iam_resolve_helpers || true)"
if [[ -z "$_AUTH_HELPERS" ]]; then
  printf '%s\n' "gcloud-iam: sibling helpers workflow/gcloud-auth/gcloud.sh not found — install workflow/gcloud-auth alongside this plugin." >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$_AUTH_HELPERS"

PLUGIN="gcloud-iam"

# ── helpers ─────────────────────────────────────────────────────────────────

_gcloud_iam_field() {
  printf '%s' "$1" | jq -r --arg k "$2" '.[$k] // empty' 2>/dev/null || true
}

# Resolve the project: explicit value wins; else the local `gcloud config` one.
_gcloud_iam_resolve_project() {
  local p="$1"
  if [[ -n "$p" ]]; then printf '%s' "$p"; return 0; fi
  gcloud config get-value project 2>/dev/null | head -n1 | tr -d '[:space:]'
}

# ── verbs ─────────────────────────────────────────────────────────────────

_gcloud_iam_get() {
  local request="$1"
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local project
  project="$(_gcloud_iam_resolve_project "$(_gcloud_iam_field "$request" project)")"
  if [[ -z "$project" ]]; then
    printf '%s\n' "$PLUGIN get: no project (pass .project or set a gcloud default project)" >&2
    exit 2
  fi

  local -a args=(projects get-iam-policy --format=json -- "$project")
  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN get: 'gcloud projects get-iam-policy $project' failed: $(printf '%s' "$raw" | head -c 300)" >&2
    exit 6
  fi
  printf '%s' "$raw" | jq --arg p "$project" '{
    ok: true,
    plugin: "gcloud-iam",
    operation: "get",
    project: $p,
    bindings: [ (.bindings // [])[] | { role: .role, members: (.members // []) } ]
  }'
}

_gcloud_iam_binding() {
  local request="$1" op="$2"   # op = add | remove
  gcloud_assert_cli || exit 1
  gcloud_assert_authenticated || exit 3

  local project member role
  project="$(_gcloud_iam_field "$request" project)"
  member="$(_gcloud_iam_field "$request" member)"
  role="$(_gcloud_iam_field "$request" role)"
  if [[ -z "$project" || -z "$member" || -z "$role" ]]; then
    printf '%s\n' "$PLUGIN $op: .project, .member and .role are all required" >&2
    exit 2
  fi

  local gverb
  case "$op" in
    add)    gverb="add-iam-policy-binding" ;;
    remove) gverb="remove-iam-policy-binding" ;;
    *) printf '%s\n' "$PLUGIN: bad binding op '$op'" >&2; exit 2 ;;
  esac

  # member/role bound as --flag=value; project positional after `--`.
  local -a args=(projects "$gverb" "--member=${member}" "--role=${role}" --format=json -- "$project")
  local raw
  if ! raw="$(gcloud "${args[@]}" 2>&1)"; then
    printf '%s\n' "$PLUGIN $op: 'gcloud projects $gverb $project' failed: $(printf '%s' "$raw" | head -c 400)" >&2
    exit 6
  fi
  # add/remove-iam-policy-binding return the updated policy; report the grant
  # plus whether the (member, role) pair is present in the result.
  printf '%s' "$raw" | jq --arg p "$project" --arg m "$member" --arg r "$role" --arg op "$op" '{
    ok: true,
    plugin: "gcloud-iam",
    operation: $op,
    project: $p,
    member: $m,
    role: $r,
    present: ([ (.bindings // [])[] | select(.role == $r) | (.members // [])[] ] | index($m) != null)
  }'
}

# ── dispatch ───────────────────────────────────────────────────────────────

_gcloud_iam_main() {
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
    get)    _gcloud_iam_get "$request" ;;
    add)    _gcloud_iam_binding "$request" add ;;
    remove) _gcloud_iam_binding "$request" remove ;;
    *)
      printf '%s\n' "$PLUGIN: unknown operation '$operation' (expected get | add | remove)" >&2
      exit 2 ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _gcloud_iam_main
fi
