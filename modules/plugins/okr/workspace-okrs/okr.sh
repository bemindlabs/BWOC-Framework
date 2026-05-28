#!/usr/bin/env bash
#
# workspace-okrs — okr/workspace-okrs plugin entry (BWOC-49).
#
# A reference `okr`-kind plugin. Reads operator-authored Objectives + Key
# Results from the sibling `objectives.toml` / `key_results.toml` and reports
# progress. LOCAL-FILE-ONLY: no network, no credentials, no external system of
# record. The one write verb (`track`) updates the operator's own
# `key_results.toml` — a tracked file the operator can `git diff` — so it
# carries no confirmation gate (notes/2026-05-28_okr-plugin-architecture.md §3).
#
# Verbs (notes/2026-05-28_okr-plugin-architecture.md §3):
#   track          --key-result <id> --current <value> [--evidence <kind:value>]
#                  Updates `current` (+ evidence, + as_of) for one KR in
#                  key_results.toml. Emits the updated KR as a progress entry.
#   check-progress Emits per-KR status (on-track / at-risk / off-track) plus a
#                  per-objective rollup. Read-only.
#   report         Emits the full OKR Progress Schema JSON for every KR
#                  (docs/en/PLUGINS.en.md §OKR Progress Schema). Read-only.
#
# Verb resolution: the first argument, or $BWOC_OKR_OPERATION when no argument
# is given. The `bwoc okr` dispatcher (BWOC-48) spawns this entry with NO argv,
# sets BWOC_OKR_OPERATION, and pipes a one-line JSON request on stdin
# ({"operation":"track","key_result_id":"...","current":N,"evidence":"..."}).
# `track` therefore reads its parameters from that stdin JSON when present, and
# falls back to argv flags for hand-invocation / smoke tests. Data files resolve
# from $BWOC_PLUGIN_DIR, falling back to this script's own directory.
#
# Exit codes:
#   0  success — one JSON document on stdout
#   1  dependency / IO error (jq missing, data file missing or malformed)
#   2  usage error (unknown verb, missing/invalid flag, unknown key result)
#
# A non-JSON-clean failure prints a human diagnostic on stderr and exits
# non-zero; the plugin never panics on missing or malformed TOML.

set -euo pipefail

PLUGIN="workspace-okrs"

# Attainment threshold for the on-track band. A documented, intentionally
# simple heuristic (notes/2026-05-28_okr-plugin-architecture.md §3): the
# time-phased "expected attainment by period elapsed" model is deferred, so v1
# uses a single flat line — the canonical OKR "green" threshold.
EXPECTED_ATTAINMENT="0.7"

die() { printf '%s: %s\n' "$PLUGIN" "$1" >&2; exit "${2:-1}"; }

require_jq() {
  command -v jq >/dev/null 2>&1 || die "required command 'jq' not found on PATH — install jq, then retry." 1
}

# ── data resolution ────────────────────────────────────────────────────────

if [[ -z "${BWOC_PLUGIN_DIR:-}" ]]; then
  BWOC_PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
DATA_DIR="$BWOC_PLUGIN_DIR"
OBJECTIVES_FILE="$DATA_DIR/objectives.toml"
KEY_RESULTS_FILE="$DATA_DIR/key_results.toml"

# ── TOML parsing ─────────────────────────────────────────────────────────────
#
# objectives.toml / key_results.toml use a constrained shape: array-of-tables
# headers (`[[objective]]` / `[[key_result]]`), single-line scalar assignments,
# and a single-line inline `evidence` table. awk walks each file and emits one
# TSV row per record in declaration order. Authored values never contain a tab
# (the field separator) — a tab in a string would corrupt the row, which the
# numeric/enum guards below surface rather than pass through silently.

# parse_key_results — one TSV row per key result:
#   id \t objective_id \t description \t target \t current \t unit \t
#   confidence \t evidence_kind \t evidence_value \t as_of
parse_key_results() {
  awk '
    function strval(line) { sub(/^[^=]*=[ \t]*/, "", line); gsub(/^"|"[ \t]*$/, "", line); return line }
    function numval(line) { sub(/^[^=]*=[ \t]*/, "", line); gsub(/[ \t]+$/, "", line); return line }
    function emit() {
      if (id == "") return
      printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", \
        id, oid, desc, target, current, unit, confidence, evkind, evval, asof
    }
    /^[ \t]*#/ { next }
    /^[ \t]*$/ { next }
    /^\[\[key_result\]\]/ {
      emit()
      id=""; oid=""; desc=""; target=""; current=""; unit=""; confidence=""; evkind="none"; evval=""; asof=""
      next
    }
    /^[ \t]*key_result_id[ \t]*=/ { id=strval($0); next }
    /^[ \t]*objective_id[ \t]*=/  { oid=strval($0); next }
    /^[ \t]*description[ \t]*=/   { desc=strval($0); next }
    /^[ \t]*target[ \t]*=/        { target=numval($0); next }
    /^[ \t]*current[ \t]*=/       { current=numval($0); next }
    /^[ \t]*unit[ \t]*=/          { unit=strval($0); next }
    /^[ \t]*confidence[ \t]*=/    { confidence=strval($0); next }
    /^[ \t]*evidence[ \t]*=/ {
      if (match($0, /kind[ \t]*=[ \t]*"[^"]*"/)) {
        k=substr($0, RSTART, RLENGTH); sub(/^kind[ \t]*=[ \t]*"/, "", k); sub(/"$/, "", k); evkind=k
      }
      if (match($0, /value[ \t]*=[ \t]*"[^"]*"/)) {
        v=substr($0, RSTART, RLENGTH); sub(/^value[ \t]*=[ \t]*"/, "", v); sub(/"$/, "", v); evval=v
      }
      next
    }
    /^[ \t]*as_of[ \t]*=/         { asof=strval($0); next }
    END { emit() }
  ' "$KEY_RESULTS_FILE"
}

# parse_objectives — one TSV row per objective:
#   objective_id \t title \t owner \t period \t parent
parse_objectives() {
  awk '
    function strval(line) { sub(/^[^=]*=[ \t]*/, "", line); gsub(/^"|"[ \t]*$/, "", line); return line }
    function emit() {
      if (oid == "") return
      printf "%s\t%s\t%s\t%s\t%s\n", oid, title, owner, period, parent
    }
    /^[ \t]*#/ { next }
    /^[ \t]*$/ { next }
    /^\[\[objective\]\]/ { emit(); oid=""; title=""; owner=""; period=""; parent=""; next }
    /^[ \t]*objective_id[ \t]*=/ { oid=strval($0); next }
    /^[ \t]*title[ \t]*=/        { title=strval($0); next }
    /^[ \t]*owner[ \t]*=/        { owner=strval($0); next }
    /^[ \t]*period[ \t]*=/       { period=strval($0); next }
    /^[ \t]*parent[ \t]*=/       { parent=strval($0); next }
    END { emit() }
  ' "$OBJECTIVES_FILE"
}

# ── input sanity (runtime; the authoritative static validator is BWOC-50) ────
#
# Prevents panics and garbage JSON on malformed input: required fields present,
# target/current numeric. Deep referential + enum validation belongs to the
# `bwoc check` extension (BWOC-50), not the runtime path.
assert_key_results_ok() {
  [[ -f "$KEY_RESULTS_FILE" ]] || die "missing $KEY_RESULTS_FILE" 1
  local rows; rows="$(parse_key_results)"
  [[ -n "$rows" ]] || die "no key results found in $KEY_RESULTS_FILE (empty or malformed)" 1
  local id oid desc target current unit confidence evkind evval asof
  while IFS=$'\t' read -r id oid desc target current unit confidence evkind evval asof; do
    [[ -z "$id" ]] && continue
    [[ -n "$oid" ]]    || die "key result '$id' has no objective_id" 1
    [[ -n "$unit" ]]   || die "key result '$id' has no unit" 1
    [[ "$target"  =~ ^-?[0-9]+(\.[0-9]+)?$ ]] || die "key result '$id' has non-numeric target '$target'" 1
    [[ "$current" =~ ^-?[0-9]+(\.[0-9]+)?$ ]] || die "key result '$id' has non-numeric current '$current'" 1
  done <<< "$rows"
}

# compute_kr_status — TSV: key_result_id \t objective_id \t attainment \t status
# Heuristic (design note §3): attainment = current / target; on-track when
# attainment >= EXPECTED_ATTAINMENT OR confidence == high; else at-risk when
# confidence == medium; else off-track. boolean units use 0/1 for current.
compute_kr_status() {
  parse_key_results | awk -F'\t' -v exp="$EXPECTED_ATTAINMENT" '
    {
      id=$1; oid=$2; target=$4+0; current=$5+0; conf=$7;
      if (target == 0) { att = (current >= target) ? 1.0 : 0.0 } else { att = current / target }
      if (att >= exp+0 || conf == "high") st="on-track";
      else if (conf == "medium")          st="at-risk";
      else                                st="off-track";
      printf "%s\t%s\t%.4f\t%s\n", id, oid, att, st;
    }'
}

# ── verbs ────────────────────────────────────────────────────────────────────

cmd_report() {
  require_jq
  assert_key_results_ok
  parse_key_results | jq -R -s '
    split("\n") | map(select(length > 0) | split("\t")) |
    map(
      {
        objective_id:  .[1],
        key_result_id: .[0],
        target:        (.[3] | tonumber),
        current:       (.[4] | tonumber),
        unit:          .[5],
        confidence:    .[6],
        evidence:      { kind: .[7], value: .[8] }
      }
      + (if (.[9] | length) > 0 then { as_of: .[9] } else {} end)
    )'
}

cmd_check_progress() {
  require_jq
  [[ -f "$OBJECTIVES_FILE" ]] || die "missing $OBJECTIVES_FILE" 1
  assert_key_results_ok

  local kr_status kr_json obj_json
  kr_status="$(compute_kr_status)"

  kr_json="$(printf '%s\n' "$kr_status" | jq -R -s -c '
    split("\n") | map(select(length > 0) | split("\t")) |
    map({ key_result_id: .[0], objective_id: .[1], attainment: (.[2] | tonumber), status: .[3] })')"

  # Per-objective rollup, in objectives.toml declaration order. Status rolls up
  # to the worst KR status (off-track > at-risk > on-track).
  obj_json="$(parse_objectives | awk -F'\t' -v ks="$kr_status" '
    BEGIN {
      n = split(ks, lines, "\n");
      for (i = 1; i <= n; i++) {
        if (lines[i] == "") continue;
        split(lines[i], f, "\t"); oid=f[2]; st=f[4];
        total[oid]++;
        if (st == "on-track") on[oid]++; else if (st == "at-risk") risk[oid]++; else off[oid]++;
        r = (st == "off-track") ? 3 : ((st == "at-risk") ? 2 : 1);
        if (r > worst[oid]) worst[oid] = r;
      }
    }
    {
      oid=$1; title=$2; w=worst[oid];
      ws = (w == 3) ? "off-track" : ((w == 2) ? "at-risk" : "on-track");
      printf "%s\t%s\t%s\t%d\t%d\t%d\t%d\n", oid, title, ws, on[oid]+0, risk[oid]+0, off[oid]+0, total[oid]+0;
    }' | jq -R -s -c '
      split("\n") | map(select(length > 0) | split("\t")) |
      map({
        objective_id: .[0],
        title:        .[1],
        status:       .[2],
        counts:       { on_track: (.[3]|tonumber), at_risk: (.[4]|tonumber), off_track: (.[5]|tonumber), total: (.[6]|tonumber) }
      })')"

  jq -n \
    --argjson krs "$kr_json" \
    --argjson objs "$obj_json" \
    --argjson exp "$EXPECTED_ATTAINMENT" \
    '{ plugin: "workspace-okrs", operation: "check-progress", expected_attainment: $exp, key_results: $krs, objectives: $objs }'
}

cmd_track() {
  require_jq
  assert_key_results_ok

  local krid="" newcur="" ev=""

  # Two input paths, in priority order:
  #   1. A one-line JSON request on stdin — the `bwoc okr` dispatcher (BWOC-48)
  #      spawns this entry with no argv, sets BWOC_OKR_OPERATION=track, and pipes
  #      {"operation":"track","key_result_id":"...","current":N,"evidence":"..."}.
  #   2. argv flags — for hand-invocation / smoke tests.
  # Read stdin only when it is not a terminal, so an interactive `okr.sh track
  # --key-result ...` never blocks waiting on stdin.
  local request=""
  if [[ ! -t 0 ]]; then request="$(cat || true)"; fi
  if [[ -n "$request" ]] && printf '%s' "$request" | jq -e 'type == "object"' >/dev/null 2>&1; then
    krid="$(printf '%s' "$request" | jq -r '.key_result_id // empty')"
    newcur="$(printf '%s' "$request" | jq -r 'if .current == null then "" else (.current | tostring) end')"
    ev="$(printf '%s' "$request" | jq -r '.evidence // empty')"
  else
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --key-result) krid="${2:-}"; shift 2 || die "track: --key-result needs a value" 2 ;;
        --current)    newcur="${2:-}"; shift 2 || die "track: --current needs a value" 2 ;;
        --evidence)   ev="${2:-}"; shift 2 || die "track: --evidence needs a value" 2 ;;
        *) die "track: unknown flag '$1' (expected --key-result | --current | --evidence)" 2 ;;
      esac
    done
  fi

  [[ -n "$krid" ]]   || die "track: key_result_id is required (--key-result <id> or stdin .key_result_id)" 2
  [[ -n "$newcur" ]] || die "track: current value is required (--current <value> or stdin .current)" 2
  [[ "$newcur" =~ ^-?[0-9]+(\.[0-9]+)?$ ]] || die "track: current must be numeric, got '$newcur'" 2

  local setev=0 evkind="" evval=""
  if [[ -n "$ev" ]]; then
    setev=1
    evkind="${ev%%:*}"
    if [[ "$ev" == *:* ]]; then evval="${ev#*:}"; else evval=""; fi
    case "$evkind" in
      file|content|command|attestation|sample|none) ;;
      *) die "track: --evidence kind must be one of file|content|command|attestation|sample|none, got '$evkind'" 2 ;;
    esac
    [[ "$evkind" == "none" ]] && evval=""
    [[ "$evval" == *'"'* ]] && die "track: --evidence value must not contain a double-quote" 2
  fi

  # Key result must exist. grep over a captured string (no upstream pipe to
  # break, so no SIGPIPE under pipefail).
  local ids; ids="$(parse_key_results | cut -f1)"
  grep -qxF "$krid" <<< "$ids" || die "track: key result '$krid' not found in $KEY_RESULTS_FILE" 2

  local asof; asof="$(date -u +%Y-%m-%d)"

  # Rewrite the target block: replace `current`, optionally `evidence`, and
  # re-stamp `as_of` (dropping any existing as_of, re-emitting it right after
  # evidence). Reconstructs lines via index/substr rather than sub() so an `&`
  # in an evidence value is never reinterpreted. Atomic via temp + mv.
  local tmp; tmp="$(mktemp -t workspace-okrs.XXXXXX)"
  awk -v id="$krid" -v newcur="$newcur" -v setev="$setev" -v evk="$evkind" -v evv="$evval" -v asof="$asof" '
    function strval(line) { sub(/^[^=]*=[ \t]*/, "", line); gsub(/^"|"[ \t]*$/, "", line); return line }
    /^\[\[key_result\]\]/ { intarget=0; print; next }
    /^[ \t]*key_result_id[ \t]*=/ { intarget = (strval($0) == id) ? 1 : 0; print; next }
    {
      if (intarget) {
        if ($0 ~ /^[ \t]*current[ \t]*=/) { pre=substr($0, 1, index($0, "=")); print pre " " newcur; next }
        if ($0 ~ /^[ \t]*as_of[ \t]*=/)   { next }   # dropped; re-emitted after evidence
        if ($0 ~ /^[ \t]*evidence[ \t]*=/) {
          pre = substr($0, 1, index($0, "="));
          if (setev == "1") { print pre " { kind = \"" evk "\", value = \"" evv "\" }" } else { print }
          print "as_of         = \"" asof "\"";
          next
        }
      }
      print
    }
  ' "$KEY_RESULTS_FILE" > "$tmp" || { rm -f "$tmp"; die "track: failed to rewrite $KEY_RESULTS_FILE" 1; }
  mv "$tmp" "$KEY_RESULTS_FILE"

  # Emit the updated KR as a single progress entry (design note §3: "the
  # updated KR row").
  parse_key_results | jq -R -s --arg id "$krid" '
    split("\n") | map(select(length > 0) | split("\t")) |
    map(select(.[0] == $id)) | .[0] |
    {
      objective_id:  .[1],
      key_result_id: .[0],
      target:        (.[3] | tonumber),
      current:       (.[4] | tonumber),
      unit:          .[5],
      confidence:    .[6],
      evidence:      { kind: .[7], value: .[8] }
    }
    + (if (.[9] | length) > 0 then { as_of: .[9] } else {} end)'
}

# ── dispatch ─────────────────────────────────────────────────────────────────

main() {
  local verb=""
  if [[ $# -gt 0 ]]; then verb="$1"; shift; else verb="${BWOC_OKR_OPERATION:-}"; fi

  case "$verb" in
    track)          cmd_track "$@" ;;
    check-progress) cmd_check_progress ;;
    report)         cmd_report ;;
    "") die "usage: okr.sh {track|check-progress|report} [flags]  (or set BWOC_OKR_OPERATION)" 2 ;;
    *)  die "unknown verb '$verb' (expected track | check-progress | report)" 2 ;;
  esac
}

# Only dispatch when executed directly; sourcing imports the helpers cleanly.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
