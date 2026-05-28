#!/usr/bin/env bash
#
# gcloud-auth — workflow/gcloud-auth plugin entry + shared helpers (BWOC-53).
#
# Two roles in one file:
#
#   1. As an ENTRY SCRIPT (executed directly): dispatches the `status` and
#      `login` verbs per BWOC_GCLOUD_OPERATION + the one-line JSON request on
#      stdin. Emits one JSON object on stdout on success; a human diagnostic
#      on stderr + non-zero exit on error.
#
#   2. As a HELPER LIBRARY (sourced by sibling gcloud-* plugins): exports
#      gcloud_assert_cli / gcloud_active_source / gcloud_account_email /
#      gcloud_assert_authenticated for credential-state introspection. The
#      dispatcher is BASH_SOURCE-guarded so sourcing is a pure import.
#
# Contract:
#   stdin                  one-line JSON, e.g. {"operation":"status"}
#                          or {"operation":"login","account":"me@example.com"}
#   BWOC_GCLOUD_OPERATION  fallback for .operation when stdin is empty
#   BWOC_WORKSPACE         absolute workspace root (resolves the SA JSON path)
#   BWOC_PLUGIN_DIR        absolute path to this plugin directory (informational)
#
# Security (Sila — Adinnaadana):
#   The plugin NEVER reads any credential value. It checks file presence and
#   asks `gcloud` for state. The token is never echoed, never written, never
#   placed in any JSON output. auth.toml ships SHAPE only — no values.

set -euo pipefail

# When sourced, sibling plugins override PLUGIN to their own identifier AFTER
# the source line — so this default only affects the dispatcher in this file.
PLUGIN="gcloud-auth"

# Auth source precedence (decision 3 of the BWOC-51 design note).
ADC_PATH_DEFAULT="${HOME}/.config/gcloud/application_default_credentials.json"
SA_PATH_RELATIVE=".bwoc/secrets/gcloud-sa.json"

# ── helpers (safe to source) ───────────────────────────────────────────────

# gcloud_assert_cli — fails fast (exit 127 from the caller) when `gcloud` is
# missing. Returns instead of exiting so callers can choose their own code.
gcloud_assert_cli() {
  if ! command -v gcloud >/dev/null 2>&1; then
    printf '%s\n' "$PLUGIN: required command 'gcloud' not found on PATH — install Google Cloud SDK, then retry." >&2
    return 127
  fi
  return 0
}

# gcloud_adc_path — absolute path to the active ADC JSON, if present. Empty
# string when absent. Honors $CLOUDSDK_CONFIG.
gcloud_adc_path() {
  local p="${CLOUDSDK_CONFIG:+$CLOUDSDK_CONFIG/application_default_credentials.json}"
  [[ -z "$p" ]] && p="$ADC_PATH_DEFAULT"
  if [[ -f "$p" ]]; then printf '%s' "$p"; fi
}

# gcloud_sa_path — absolute path to the workspace-local service-account JSON,
# if present. Empty string when absent.
gcloud_sa_path() {
  local root="${BWOC_WORKSPACE:-.}"
  local p="${root%/}/${SA_PATH_RELATIVE}"
  if [[ -f "$p" ]]; then printf '%s' "$p"; fi
}

# gcloud_active_source — echoes one of: adc | service-account | env | none.
# Precedence: ADC -> service_account -> env -> none.
gcloud_active_source() {
  if [[ -n "$(gcloud_adc_path)" ]]; then printf 'adc'; return 0; fi
  if [[ -n "$(gcloud_sa_path)"  ]]; then printf 'service-account'; return 0; fi
  if [[ -n "${BWOC_GCLOUD_ACCOUNT:-}${BWOC_GCLOUD_PROJECT:-}${BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT:-}" ]]; then
    printf 'env'; return 0
  fi
  printf 'none'
}

# gcloud_account_email — active account email per local `gcloud`. Empty when
# unauthenticated. Never prints the token. Quiet on stderr (gcloud writes
# "(unset)" to stderr in newer versions).
gcloud_account_email() {
  gcloud_assert_cli || return $?
  local email
  email="$(gcloud config get-value account 2>/dev/null || true)"
  if [[ "$email" == "(unset)" || -z "${email// }" ]]; then return 0; fi
  printf '%s' "$email"
}

# gcloud_assert_authenticated — exit-style guard. Returns 3 with a clear
# diagnostic when there is no active credential. Idempotent on replay.
gcloud_assert_authenticated() {
  local email
  email="$(gcloud_account_email || true)"
  if [[ -z "$email" ]]; then
    printf '%s\n' "$PLUGIN: no active gcloud account — run 'gcloud auth login' or 'gcloud auth application-default login'." >&2
    return 3
  fi
  return 0
}

# ── verbs (executed only when this file is the entry script) ───────────────

_gcloud_auth_status() {
  # `status` does not REQUIRE auth — it reports state including "unauthenticated".
  # It does require `gcloud` for the `account` lookup; if that's missing we
  # still emit a structured envelope rather than panicking.
  local source email adc_path sa_path has_cred env_present
  source="$(gcloud_active_source)"
  adc_path="$(gcloud_adc_path)"
  sa_path="$(gcloud_sa_path)"
  email=""
  if command -v gcloud >/dev/null 2>&1; then
    email="$(gcloud_account_email || true)"
  fi

  has_cred=false
  if [[ "$source" != "none" || -n "$email" ]]; then has_cred=true; fi

  env_present=false
  if [[ -n "${BWOC_GCLOUD_ACCOUNT:-}${BWOC_GCLOUD_PROJECT:-}${BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT:-}" ]]; then
    env_present=true
  fi

  jq -n \
    --arg src "$source" \
    --arg email "$email" \
    --argjson has "$has_cred" \
    --arg adc "$adc_path" \
    --arg sa "$sa_path" \
    --argjson env_present "$env_present" \
    --argjson gcloud_present "$(command -v gcloud >/dev/null 2>&1 && echo true || echo false)" \
    '{
      ok: true,
      plugin: "gcloud-auth",
      operation: "status",
      gcloud_cli_present: $gcloud_present,
      active_source: $src,
      account_email: (if $email == "" then null else $email end),
      has_credential: $has,
      sources: {
        adc:             { present: ($adc | length > 0), path: (if $adc == "" then null else $adc end) },
        service_account: { present: ($sa  | length > 0), path: (if $sa  == "" then null else $sa  end) },
        env:             { present: $env_present, vars: ["BWOC_GCLOUD_ACCOUNT","BWOC_GCLOUD_PROJECT","BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT"] }
      }
    }'
}

_gcloud_auth_login() {
  # Operator-driven only. The plugin lets `gcloud auth login` stream to the
  # operator's TTY and emits a single telemetry line on stdout AFTER it exits.
  # Never auto-invoked by an agent (the gcloud-ops skill explicitly excludes
  # this verb — BWOC-51 design note §Decision 5).
  local request="$1"
  local account
  account="$(printf '%s' "$request" | jq -r '.account // empty' 2>/dev/null || true)"

  gcloud_assert_cli || exit 1

  if [[ -n "$account" ]]; then
    # `--` ends option parsing: a `-`-leading account is a positional, never a
    # gcloud flag (option-injection guard, #91).
    gcloud auth login --brief -- "$account"
  else
    gcloud auth login --brief
  fi

  local now
  now="$(gcloud_account_email || true)"
  jq -n --arg email "$now" '{
    ok: true,
    plugin: "gcloud-auth",
    operation: "login",
    account_email: (if $email == "" then null else $email end)
  }'
}

_gcloud_auth_main() {
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
    status) _gcloud_auth_status ;;
    login)  _gcloud_auth_login "$request" ;;
    *)
      printf '%s\n' "$PLUGIN: unknown operation '$operation' (expected status | login)" >&2
      exit 2 ;;
  esac
}

# Only dispatch when executed directly. Sourcing this file imports the helpers
# without side-effects on the caller's stdin / argv.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _gcloud_auth_main
fi
