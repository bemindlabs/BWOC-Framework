#!/usr/bin/env bash
#
# audit-iso-iec-ieee-29148 — runtime. Emits attestation findings per BWOC-27 schema.
#
# Reads operator-declared attestations from .bwoc/workspace.toml under
# [[plugins.audit-iso-iec-ieee-29148.attestations]]. Matches each entry's criterion_id
# against criteria.toml's declared criteria. Emits a pass finding (evidence.kind
# = "attestation" with signer + signed_at + optional valid_through) when an
# attestation is present; otherwise emits a fail finding pointing the operator
# at workspace.toml as the remedy.
#
# Env contract (set by `bwoc audit run` dispatcher):
#   BWOC_WORKSPACE       — absolute path to workspace root
#   BWOC_PLUGIN_DIR      — absolute path to this plugin dir
#   BWOC_AUDIT_OPERATION — expected "audit_run"
#
# Schema: PLUGINS.en.md §Audit Findings Schema (BWOC-11 + BWOC-27 extension).
# Design: the shared attestation runtime (notes/2026-05-27_9001-runtime-attestation-source.md); this lane reuses it for ISO/IEC/IEEE 29148.
#
# Exit 0 on success — non-pass findings are *findings*, not errors. A
# non-zero exit signals a framework-side problem (unreadable criteria.toml).

set -euo pipefail

# -- Resolve inputs ---------------------------------------------------------

if [[ -z "${BWOC_WORKSPACE:-}" ]]; then
  echo "audit-iso-iec-ieee-29148: BWOC_WORKSPACE is unset" >&2
  exit 1
fi
if [[ -z "${BWOC_PLUGIN_DIR:-}" ]]; then
  BWOC_PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
WORKSPACE="$BWOC_WORKSPACE"
CRITERIA_FILE="$BWOC_PLUGIN_DIR/criteria.toml"
WORKSPACE_TOML="$WORKSPACE/.bwoc/workspace.toml"

if [[ ! -f "$CRITERIA_FILE" ]]; then
  echo "audit-iso-iec-ieee-29148: missing $CRITERIA_FILE" >&2
  exit 1
fi

# -- Parse criteria.toml ----------------------------------------------------
#
# Emit one TSV row per criterion in declaration order: <id>\t<severity>.

parse_criteria() {
  awk '
    function emit() {
      if (id != "") printf "%s\t%s\n", id, severity
    }
    /^[ \t]*#/ { next }
    /^[ \t]*$/ { next }
    /^\[criterion\./ {
      emit()
      id = $0
      sub(/^\[criterion\./, "", id)
      sub(/\][ \t]*$/, "", id)
      severity = ""
      next
    }
    /^[ \t]*severity[ \t]*=/ {
      v = $0
      sub(/^[^=]*=[ \t]*"/, "", v)
      sub(/"[ \t]*$/, "", v)
      severity = v
      next
    }
    END { emit() }
  ' "$CRITERIA_FILE"
}

# -- Parse workspace.toml [[plugins.audit-iso-iec-ieee-29148.attestations]] -----------
#
# Emit one TSV row per attestation:
#   <criterion_id>\t<signer>\t<signed_at>\t<valid_through>\t<statement>
#
# Strings are TOML basic strings (double-quoted, single-line). v0.2.0 does
# not support multi-line statements — escape literal `\n` inside the quoted
# string if needed. `valid_through` is optional → empty string if absent.
#
# Block boundary: the first `[` line after entering the block ends the
# entry (whether `[plugins.x]`, `[[plugins.x.attestations]]`, or `[other]`).
#
# Workspace.toml absent → no attestations → every criterion fails with the
# missing-attestation remedy. That is the expected first-run state.

parse_attestations() {
  if [[ ! -f "$WORKSPACE_TOML" ]]; then
    return 0
  fi
  awk '
    function string_value(line) {
      sub(/^[^=]*=[ \t]*"/, "", line)
      sub(/"[ \t]*$/, "", line)
      return line
    }
    function emit() {
      if (in_block && criterion_id != "") {
        printf "%s\t%s\t%s\t%s\t%s\n", criterion_id, signer, signed_at, valid_through, statement
      }
      criterion_id = ""; signer = ""; signed_at = ""; valid_through = ""; statement = ""
    }
    /^[ \t]*#/ { next }
    /^[ \t]*$/ { next }
    /^\[\[plugins\.audit-iso-iec-ieee-29148\.attestations\]\]/ {
      emit()
      in_block = 1
      next
    }
    /^\[/ {
      emit()
      in_block = 0
      next
    }
    in_block && /^[ \t]*criterion_id[ \t]*=/  { criterion_id  = string_value($0); next }
    in_block && /^[ \t]*signer[ \t]*=/        { signer        = string_value($0); next }
    in_block && /^[ \t]*signed_at[ \t]*=/     { signed_at     = string_value($0); next }
    in_block && /^[ \t]*valid_through[ \t]*=/ { valid_through = string_value($0); next }
    in_block && /^[ \t]*statement[ \t]*=/     { statement     = string_value($0); next }
    END { emit() }
  ' "$WORKSPACE_TOML"
}

# -- JSON helpers -----------------------------------------------------------

json_escape() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  s="${s//$'\n'/\\n}"
  s="${s//$'\t'/\\t}"
  printf '%s' "$s"
}

# -- Build attestation lookup -----------------------------------------------
#
# Bash 3.2 (macOS default) lacks associative arrays, so the lookup is a TSV
# file keyed by criterion_id. First occurrence wins (duplicate criterion_id
# is a workspace-author bug; bwoc check is the right place to surface that —
# out of scope for the runtime).

ATTESTATIONS_TSV="$(mktemp -t audit-iso-iec-ieee-29148-attestations.XXXXXX)"
trap 'rm -f "$ATTESTATIONS_TSV"' EXIT

parse_attestations > "$ATTESTATIONS_TSV"

# Lookup helper: extracts one field of the matching row for the given
# criterion_id. Field index is 2..5 (1 is the criterion_id itself, used to
# match). We extract one field at a time because bash's `read -r` with
# IFS=$'\t' collapses consecutive tabs (tab is IFS whitespace per the bash
# manual), which mangles empty optional fields. awk over a small TSV is O(N)
# per call — N is single-digit in practice, no need for a hash.

lookup_attestation_field() {
  local cid="$1" field_index="$2"
  awk -F '\t' -v target="$cid" -v idx="$field_index" '
    $1 == target {
      printf "%s", $idx
      exit
    }
  ' "$ATTESTATIONS_TSV"
}

# -- Emit findings ----------------------------------------------------------

printf '['
first=1
while IFS=$'\t' read -r id severity; do
  [[ -z "$id" ]] && continue
  if [[ $first -eq 1 ]]; then first=0; else printf ','; fi

  # Probe whether an attestation row exists for this criterion; lookup the
  # criterion_id field itself to disambiguate "row absent" from "row present
  # with all-empty fields".
  have_attestation="$(lookup_attestation_field "$id" 1)"
  if [[ -n "$have_attestation" ]]; then
    a_signer="$(lookup_attestation_field "$id" 2)"
    a_signed_at="$(lookup_attestation_field "$id" 3)"
    a_valid_through="$(lookup_attestation_field "$id" 4)"
    a_statement="$(lookup_attestation_field "$id" 5)"

    # Required-field check — the schema requires non-empty statement (value),
    # signer, and signed_at. Missing any is an operator workspace.toml error;
    # surface as fail with a precise remedy.
    if [[ -z "$a_signer" || -z "$a_signed_at" || -z "$a_statement" ]]; then
      missing=""
      [[ -z "$a_statement" ]] && missing="$missing statement"
      [[ -z "$a_signer"    ]] && missing="$missing signer"
      [[ -z "$a_signed_at" ]] && missing="$missing signed_at"
      remedy="Attestation for ${id} in .bwoc/workspace.toml is missing required field(s):${missing}. Provide statement, signer, and signed_at (ISO 8601 date) to satisfy this criterion."
      printf '\n  {"criterion_id":"%s","severity":"%s","status":"fail","evidence":{"kind":"file","value":".bwoc/workspace.toml"},"remedy":"%s"}' \
        "$(json_escape "$id")" \
        "$(json_escape "$severity")" \
        "$(json_escape "$remedy")"
      continue
    fi

    # Happy path — pass finding with attestation evidence. valid_through is
    # an optional time-bounded field (BWOC-26, orthogonal to kind).
    if [[ -n "$a_valid_through" ]]; then
      printf '\n  {"criterion_id":"%s","severity":"%s","status":"pass","evidence":{"kind":"attestation","value":"%s","signer":"%s","signed_at":"%s","valid_through":"%s"}}' \
        "$(json_escape "$id")" \
        "$(json_escape "$severity")" \
        "$(json_escape "$a_statement")" \
        "$(json_escape "$a_signer")" \
        "$(json_escape "$a_signed_at")" \
        "$(json_escape "$a_valid_through")"
    else
      printf '\n  {"criterion_id":"%s","severity":"%s","status":"pass","evidence":{"kind":"attestation","value":"%s","signer":"%s","signed_at":"%s"}}' \
        "$(json_escape "$id")" \
        "$(json_escape "$severity")" \
        "$(json_escape "$a_statement")" \
        "$(json_escape "$a_signer")" \
        "$(json_escape "$a_signed_at")"
    fi
  else
    # No attestation present — fail finding with workspace.toml as evidence.
    # evidence.kind = "file" (not "attestation") keeps the schema honest:
    # workspace.toml IS the reproducible referent (operator can open it and
    # see what's missing). An empty-value attestation would violate the
    # schema rule that non-none evidence requires a non-empty value.
    remedy="Provide a signed attestation in .bwoc/workspace.toml under [[plugins.audit-iso-iec-ieee-29148.attestations]] with criterion_id=\"${id}\", statement, signer, and signed_at (ISO 8601 date) to satisfy this criterion."
    printf '\n  {"criterion_id":"%s","severity":"%s","status":"fail","evidence":{"kind":"file","value":".bwoc/workspace.toml"},"remedy":"%s"}' \
      "$(json_escape "$id")" \
      "$(json_escape "$severity")" \
      "$(json_escape "$remedy")"
  fi
done < <(parse_criteria)
printf '\n]\n'
