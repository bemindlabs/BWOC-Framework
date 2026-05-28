#!/usr/bin/env bash
#
# council-sangha-7 — council/council-sangha-7 plugin entry (BWOC-59).
#
# A reference `council`-kind plugin. It RECORDS fleet decisions through a
# multi-step protocol — propose -> discuss (rounds) -> vote -> resolve — under
# the `sangha` voting model: a decision passes only by unanimous assent of the
# quorum (abstentions allowed, dissent preserved). No tie is possible — lack of
# concord re-opens a discuss round (notes/2026-05-28_council-plugin-architecture.md
# §2/§3/§6). It is LOCAL/FLEET-ONLY: no network, no credentials, no external
# system of record.
#
# It RECORDS, it does not EXECUTE. A `binding` outcome is noted as a `bwoc task`
# the fleet should carry out — the plugin never mutates code or config itself
# (design note §5). Coordination kind, not execution kind.
#
# Verbs (design note §2/§7), operating on a decision record conforming to the
# Council Decision Schema (docs/en/PLUGINS.en.md §Council Decision Schema):
#   propose  --decision-id <id> (--template <tid> | --question <q> --options a,b)
#            [--team <id> | --participants a,b] [--effect advisory|binding]
#            [--evidence <kind:value> ...]
#            Opens a decision (status=proposed). Participants/options are fixed
#            at propose time. Refuses to clobber an existing decision id.
#   discuss  --decision-id <id> --participant <agent> --message-ref <msg-id>
#            [--round <n>]
#            Appends a turn { participant, message_ref } to a round. The inbox
#            is the transport (the `bwoc send` envelope id is the message_ref);
#            the record references it, never copies it. Append-only.
#   vote     --decision-id <id> --participant <agent>
#            (--option <opt> | --abstain) [--rationale <text>]
#            Appends one vote { participant, option, abstain }. Append-only; a
#            re-cast appends and the latest wins at tally. An abstention with a
#            --rationale is preserved as dissent on resolve.
#   resolve  --decision-id <id>
#            Tallies the latest vote per participant under the sangha rule,
#            checks quorum (from [council].quorum in the manifest), records the
#            outcome + any dissent, and closes (status=resolved) — or abandons
#            (quorum not met) — or re-opens a discuss round (no concord).
#   list     Lists all decision records (summary) in records-directory order.
#   show     --decision-id <id>   (or:  show <id>)   Full decision record JSON.
#
# Verb resolution: the first argument, or $BWOC_COUNCIL_OPERATION when no
# argument is given. A `bwoc council` dispatcher (BWOC-58) may also pipe a
# one-line JSON request on stdin; argv flags override stdin fields. Templates
# resolve from $BWOC_PLUGIN_DIR/decisions.toml. Decision records persist as JSON
# under the records directory:
#   1. $BWOC_COUNCIL_DIR                       (explicit override)
#   2. $BWOC_WORKSPACE/.bwoc/council           (when a workspace is in context)
#   3. $BWOC_PLUGIN_DIR/records                (plugin-local; hand-invoke/smoke)
#
# Exit codes:
#   0  success — one JSON document on stdout
#   1  dependency / IO error (jq missing, malformed record/manifest, IO failure)
#   2  usage error (unknown verb, missing/invalid flag, unknown id/participant)
#
# Missing team / malformed record / malformed manifest fail GRACEFULLY: a clear
# diagnostic on stderr and a non-zero exit; the plugin never panics.

set -euo pipefail

PLUGIN="council-sangha-7"

die() { printf '%s: %s\n' "$PLUGIN" "$1" >&2; exit "${2:-1}"; }

require_jq() {
  command -v jq >/dev/null 2>&1 || die "required command 'jq' not found on PATH — install jq, then retry." 1
}

# ── paths ────────────────────────────────────────────────────────────────────

if [[ -z "${BWOC_PLUGIN_DIR:-}" ]]; then
  BWOC_PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
MANIFEST="$BWOC_PLUGIN_DIR/manifest.toml"
TEMPLATES_FILE="$BWOC_PLUGIN_DIR/decisions.toml"

resolve_records_dir() {
  if [[ -n "${BWOC_COUNCIL_DIR:-}" ]]; then
    printf '%s' "$BWOC_COUNCIL_DIR"
  elif [[ -n "${BWOC_WORKSPACE:-}" ]]; then
    printf '%s' "$BWOC_WORKSPACE/.bwoc/council"
  else
    printf '%s' "$BWOC_PLUGIN_DIR/records"
  fi
}
RECORDS_DIR="$(resolve_records_dir)"

now_iso() { date -u +%Y-%m-%dT%H:%M:%SZ; }

record_path() { printf '%s/%s.json' "$RECORDS_DIR" "$1"; }

# decision_id sanity: non-empty, no path separators, no leading dot.
assert_valid_id() {
  case "$1" in
    "" )       die "decision_id must be non-empty" 2 ;;
    .* )       die "decision_id '$1' must not start with a dot" 2 ;;
    */*|*\\* ) die "decision_id '$1' must not contain path separators" 2 ;;
  esac
}

# ── record IO (atomic; never partial) ─────────────────────────────────────────

load_record() {  # id -> JSON on stdout; die if missing/malformed
  local id="$1" path
  path="$(record_path "$id")"
  [[ -f "$path" ]] || die "no decision '$id' (expected $path) — propose it first" 2
  jq -e . "$path" >/dev/null 2>&1 || die "decision '$id' record is malformed JSON ($path)" 1
  cat "$path"
}

write_record() {  # id, JSON on stdin -> atomic write (temp + mv)
  local id="$1" path tmp
  mkdir -p "$RECORDS_DIR" || die "cannot create records directory $RECORDS_DIR" 1
  path="$(record_path "$id")"
  tmp="$(mktemp -t council-sangha-7.XXXXXX)" || die "mktemp failed" 1
  cat > "$tmp"
  jq -e . "$tmp" >/dev/null 2>&1 || { rm -f "$tmp"; die "refusing to persist malformed record for '$id'" 1; }
  mv "$tmp" "$path" || { rm -f "$tmp"; die "failed to write $path" 1; }
}

# ── manifest reads (voting_model / quorum live only under [council]) ──────────

manifest_field() {  # field-name -> value (quotes stripped); empty if absent
  local line
  line="$(grep -E "^[[:space:]]*$1[[:space:]]*=" "$MANIFEST" 2>/dev/null | head -1 || true)"
  [[ -n "$line" ]] || return 0
  printf '%s' "$line" | sed -E 's/^[^=]*=[[:space:]]*//; s/[[:space:]]*$//; s/^"//; s/"$//'
}

# ── decisions.toml template lookup ────────────────────────────────────────────
#
# Emits the matched template's fields as `field<TAB>value` rows; exits 3 when the
# template_id is not found. `options` is a TOML array literal (`["a","b"]`) which
# is already valid JSON, so it passes straight through to jq downstream.
template_fields() {  # template_id -> "question\t<q>" + "options\t<json-array>"
  awk -v want="$1" '
    function strval(line){ sub(/^[^=]*=[ \t]*/,"",line); gsub(/^"|"[ \t]*$/,"",line); return line }
    /^[ \t]*\[\[template\]\]/ { cur=0; next }
    /^[ \t]*template_id[ \t]*=/ { cur=(strval($0)==want); next }
    cur && /^[ \t]*question[ \t]*=/ { printf "question\t%s\n", strval($0); next }
    cur && /^[ \t]*options[ \t]*=/  { line=$0; sub(/^[^=]*=[ \t]*/,"",line); printf "options\t%s\n", line; got=1; next }
    END { if (!got) exit 3 }
  ' "$TEMPLATES_FILE"
}

# ── team roster resolution ─────────────────────────────────────────────────────

resolve_workspace() {  # echoes the workspace root, or empty
  if [[ -n "${BWOC_WORKSPACE:-}" ]]; then printf '%s' "$BWOC_WORKSPACE"; return; fi
  local d="$PWD"
  while [[ "$d" != "/" ]]; do
    if [[ -f "$d/.bwoc/workspace.toml" ]]; then printf '%s' "$d"; return; fi
    d="$(dirname "$d")"
  done
}

resolve_team_members() {  # team-id -> JSON array of member ids; die on missing/malformed
  local team="$1" ws path body tokens
  ws="$(resolve_workspace)"
  [[ -n "$ws" ]] || die "cannot resolve team '$team': no workspace context (set BWOC_WORKSPACE or pass --participants)" 2
  path="$ws/.bwoc/teams/$team.toml"
  [[ -f "$path" ]] || die "no team '$team' in workspace (expected $path) — create it with 'bwoc team create' or pass --participants" 2
  # `members = [ "a", "b", ... ]` — toml array, possibly multi-line. Capture the
  # array body, then pull each quoted token (already valid JSON strings).
  body="$(awk '
    /members[ \t]*=[ \t]*\[/ { inarr=1 }
    inarr { buf = buf $0; if ($0 ~ /\]/) inarr=0 }
    END { print buf }
  ' "$path")"
  tokens="$(printf '%s' "$body" | grep -oE '"[^"]*"' || true)"
  printf '%s' "$tokens" | jq -s '.' 2>/dev/null || die "team '$team' has a malformed members array" 1
}

# ── optional stdin JSON request (dispatcher path; argv overrides) ──────────────

STDIN_JSON=""
read_stdin_request() {
  [[ -t 0 ]] && return 0   # interactive: never block waiting on stdin
  local raw
  raw="$(cat || true)"
  if [[ -n "$raw" ]] && printf '%s' "$raw" | jq -e 'type=="object"' >/dev/null 2>&1; then
    STDIN_JSON="$raw"
  fi
}

sj() {  # field -> value from STDIN_JSON ("" if absent); arrays as JSON
  [[ -n "$STDIN_JSON" ]] || return 0
  printf '%s' "$STDIN_JSON" \
    | jq -r --arg k "$1" '(.[$k] // empty) | if type=="array" then tojson else tostring end' 2>/dev/null \
    || true
}

# ── shared builders ────────────────────────────────────────────────────────────

csv_to_json() {  # "a, b ,c" -> ["a","b","c"]
  printf '%s' "$1" | jq -R 'split(",") | map(gsub("^\\s+|\\s+$";"")) | map(select(length>0))'
}

build_evidence() {  # kind:value ... -> JSON array of audit Evidence entries
  local out="[]" kv kind val
  for kv in "$@"; do
    kind="${kv%%:*}"
    if [[ "$kv" == *:* ]]; then val="${kv#*:}"; else val=""; fi
    case "$kind" in
      file|content|command|attestation|sample|none) ;;
      *) die "evidence kind must be one of file|content|command|attestation|sample|none, got '$kind'" 2 ;;
    esac
    [[ "$kind" == "none" ]] && val=""
    out="$(printf '%s' "$out" | jq --arg k "$kind" --arg v "$val" '. + [{kind:$k, value:$v}]')"
  done
  printf '%s' "$out"
}

# ── verbs ──────────────────────────────────────────────────────────────────────

cmd_propose() {
  require_jq
  local id template question options_json team participants_csv effect stdin_participants
  id="$(sj decision_id)"; template="$(sj template)"; question="$(sj question)"
  options_json="$(sj options)"; team="$(sj team)"; effect="$(sj effect)"
  stdin_participants="$(sj participants)"; participants_csv=""
  local -a evidence_kv=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --decision-id)  id="${2:-}"; shift 2 || die "propose: --decision-id needs a value" 2 ;;
      --template)     template="${2:-}"; shift 2 || die "propose: --template needs a value" 2 ;;
      --question)     question="${2:-}"; shift 2 || die "propose: --question needs a value" 2 ;;
      --options)      options_json="$(csv_to_json "${2:-}")"; shift 2 || die "propose: --options needs a value" 2 ;;
      --team)         team="${2:-}"; shift 2 || die "propose: --team needs a value" 2 ;;
      --participants) participants_csv="${2:-}"; shift 2 || die "propose: --participants needs a value" 2 ;;
      --effect)       effect="${2:-}"; shift 2 || die "propose: --effect needs a value" 2 ;;
      --evidence)     evidence_kv+=("${2:-}"); shift 2 || die "propose: --evidence needs a value" 2 ;;
      *) die "propose: unknown flag '$1' (expected --decision-id|--template|--question|--options|--team|--participants|--effect|--evidence)" 2 ;;
    esac
  done

  [[ -n "$id" ]] || die "propose: --decision-id is required" 2
  assert_valid_id "$id"
  [[ -z "$effect" ]] && effect="advisory"
  case "$effect" in advisory|binding) ;; *) die "propose: --effect must be advisory|binding, got '$effect'" 2 ;; esac

  # template seeds question + options when they are not given explicitly
  if [[ -n "$template" ]]; then
    local tf
    tf="$(template_fields "$template")" || die "propose: no template '$template' in $TEMPLATES_FILE" 2
    [[ -n "$question" ]]     || question="$(printf '%s\n' "$tf" | awk -F'\t' '$1=="question"{sub(/^question\t/,""); print; exit}')"
    [[ -n "$options_json" ]] || options_json="$(printf '%s\n' "$tf" | awk -F'\t' '$1=="options"{sub(/^options\t/,""); print; exit}')"
  fi

  [[ -n "$question" ]]     || die "propose: --question is required (or use --template)" 2
  [[ -n "$options_json" ]] || die "propose: --options is required (or use --template)" 2
  printf '%s' "$options_json" | jq -e 'type=="array" and length>=2' >/dev/null 2>&1 \
    || die "propose: options must be a JSON array of >=2 choices, got '$options_json'" 2

  # participants: explicit csv > stdin array > team roster > empty
  local participants_json
  if [[ -n "$participants_csv" ]]; then
    participants_json="$(csv_to_json "$participants_csv")"
  elif [[ -n "$stdin_participants" ]]; then
    participants_json="$stdin_participants"
  elif [[ -n "$team" ]]; then
    participants_json="$(resolve_team_members "$team")"
  else
    participants_json="[]"
  fi
  printf '%s' "$participants_json" | jq -e 'type=="array"' >/dev/null 2>&1 \
    || die "propose: participants did not resolve to a JSON array" 1

  local evidence_json="[]"
  if [[ ${#evidence_kv[@]} -gt 0 ]]; then
    evidence_json="$(build_evidence "${evidence_kv[@]}")"
  fi

  local path; path="$(record_path "$id")"
  [[ -e "$path" ]] && die "propose: decision '$id' already exists ($path) — participants/options are fixed at propose time; choose a new id" 2

  local rec opened; opened="$(now_iso)"
  rec="$(jq -n \
    --arg id "$id" --arg q "$question" --arg eff "$effect" --arg opened "$opened" \
    --argjson opts "$options_json" --argjson parts "$participants_json" --argjson ev "$evidence_json" \
    '{
      decision_id: $id,
      status:      "proposed",
      question:    $q,
      effect:      $eff,
      participants: $parts,
      options:     $opts,
      rounds:      [],
      votes:       [],
      opened_at:   $opened
    } + (if ($ev|length) > 0 then { evidence_links: $ev } else {} end)')"

  printf '%s\n' "$rec" | write_record "$id"
  printf '%s\n' "$rec"
}

cmd_discuss() {
  require_jq
  local id participant message_ref round
  id="$(sj decision_id)"; participant="$(sj participant)"; message_ref="$(sj message_ref)"; round="$(sj round)"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --decision-id) id="${2:-}"; shift 2 || die "discuss: --decision-id needs a value" 2 ;;
      --participant) participant="${2:-}"; shift 2 || die "discuss: --participant needs a value" 2 ;;
      --message-ref) message_ref="${2:-}"; shift 2 || die "discuss: --message-ref needs a value" 2 ;;
      --round)       round="${2:-}"; shift 2 || die "discuss: --round needs a value" 2 ;;
      *) die "discuss: unknown flag '$1' (expected --decision-id|--participant|--message-ref|--round)" 2 ;;
    esac
  done

  [[ -n "$id" ]]          || die "discuss: --decision-id is required" 2
  [[ -n "$participant" ]] || die "discuss: --participant is required" 2
  [[ -n "$message_ref" ]] || die "discuss: --message-ref is required (the 'bwoc send' envelope id holding the turn)" 2

  local rec; rec="$(load_record "$id")"
  local st; st="$(printf '%s' "$rec" | jq -r .status)"
  case "$st" in resolved|abandoned) die "discuss: decision '$id' is $st (closed); no further turns" 2 ;; esac
  printf '%s' "$rec" | jq -e --arg p "$participant" '.participants | index($p) != null' >/dev/null 2>&1 \
    || die "discuss: '$participant' is not a participant of '$id'" 2

  if [[ -n "$round" ]]; then
    [[ "$round" =~ ^[0-9]+$ && "$round" -ge 1 ]] || die "discuss: --round must be a positive integer" 2
  else
    round="$(printf '%s' "$rec" | jq -r '((.rounds | map(.round) | max) // 0)')"
    [[ "$round" -ge 1 ]] 2>/dev/null || round=1
  fi

  rec="$(printf '%s' "$rec" | jq \
    --argjson rn "$round" --arg p "$participant" --arg m "$message_ref" '
    .status = (if .status=="proposed" then "discussing" else .status end)
    | (.rounds | map(.round) | index($rn)) as $idx
    | if $idx == null
        then .rounds += [{ round: $rn, turns: [{ participant: $p, message_ref: $m }] }]
        else .rounds[$idx].turns += [{ participant: $p, message_ref: $m }]
      end
    | .rounds |= sort_by(.round)')"

  printf '%s\n' "$rec" | write_record "$id"
  printf '%s\n' "$rec"
}

cmd_vote() {
  require_jq
  local id participant option abstain rationale
  id="$(sj decision_id)"; participant="$(sj participant)"; option="$(sj option)"
  rationale="$(sj rationale)"; abstain="$(sj abstain)"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --decision-id) id="${2:-}"; shift 2 || die "vote: --decision-id needs a value" 2 ;;
      --participant) participant="${2:-}"; shift 2 || die "vote: --participant needs a value" 2 ;;
      --option)      option="${2:-}"; shift 2 || die "vote: --option needs a value" 2 ;;
      --abstain)     abstain="true"; shift ;;
      --rationale)   rationale="${2:-}"; shift 2 || die "vote: --rationale needs a value" 2 ;;
      *) die "vote: unknown flag '$1' (expected --decision-id|--participant|--option|--abstain|--rationale)" 2 ;;
    esac
  done

  [[ -n "$id" ]]          || die "vote: --decision-id is required" 2
  [[ -n "$participant" ]] || die "vote: --participant is required" 2

  local rec; rec="$(load_record "$id")"
  local st; st="$(printf '%s' "$rec" | jq -r .status)"
  case "$st" in resolved|abandoned) die "vote: decision '$id' is $st (closed); no further votes" 2 ;; esac
  printf '%s' "$rec" | jq -e --arg p "$participant" '.participants | index($p) != null' >/dev/null 2>&1 \
    || die "vote: '$participant' is not a participant of '$id'" 2

  if [[ "$abstain" == "true" ]]; then
    option=""
  else
    [[ -n "$option" ]] || die "vote: --option <opt> is required unless --abstain" 2
    printf '%s' "$rec" | jq -e --arg o "$option" '.options | index($o) != null' >/dev/null 2>&1 \
      || die "vote: option '$option' is not among the decision's options" 2
  fi

  rec="$(printf '%s' "$rec" | jq \
    --arg p "$participant" --arg o "$option" --arg ab "$abstain" --arg r "$rationale" '
    .status = (if (.status=="proposed" or .status=="discussing") then "voting" else .status end)
    | .votes += [
        ( { participant: $p, abstain: ($ab=="true") }
          + (if $ab=="true" then {} else { option: $o } end)
          + (if ($r|length) > 0 then { rationale: $r } else {} end) )
      ]')"

  printf '%s\n' "$rec" | write_record "$id"
  printf '%s\n' "$rec"
}

cmd_resolve() {
  require_jq
  local id; id="$(sj decision_id)"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --decision-id) id="${2:-}"; shift 2 || die "resolve: --decision-id needs a value" 2 ;;
      *) die "resolve: unknown flag '$1' (expected --decision-id)" 2 ;;
    esac
  done
  [[ -n "$id" ]] || die "resolve: --decision-id is required" 2

  local rec; rec="$(load_record "$id")"
  local st; st="$(printf '%s' "$rec" | jq -r .status)"
  case "$st" in resolved|abandoned) die "resolve: decision '$id' is already $st" 2 ;; esac

  local quorum_raw; quorum_raw="$(manifest_field quorum)"
  [[ -n "$quorum_raw" ]] || die "resolve: manifest has no [council].quorum ($MANIFEST)" 1
  local nparts; nparts="$(printf '%s' "$rec" | jq -r '.participants | length')"
  local quorum_n
  if [[ "$quorum_raw" =~ ^[0-9]+$ ]]; then
    quorum_n="$quorum_raw"
  elif [[ "$quorum_raw" =~ ^([0-9]+)/([0-9]+)$ ]]; then
    local num den; num="${BASH_REMATCH[1]}"; den="${BASH_REMATCH[2]}"
    [[ "$den" -ne 0 ]] || die "resolve: [council].quorum fraction '$quorum_raw' has a zero denominator" 1
    quorum_n="$(awk -v n="$num" -v d="$den" -v p="$nparts" 'BEGIN{ v=n*p/d; r=int(v); if (v>r) r++; print r }')"
  else
    die "resolve: malformed [council].quorum '$quorum_raw' (expected an integer or n/m fraction)" 1
  fi

  local now result newrec; now="$(now_iso)"
  result="$(printf '%s' "$rec" | jq \
    --argjson quorum "$quorum_n" --arg now "$now" '
    (reduce .votes[] as $v ({}; .[$v.participant] = $v)) as $latest
    | ($latest | to_entries)                                  as $entries
    | ($entries | length)                                     as $voted
    | [ $entries[] | select(.value.abstain == false) ]        as $active
    | ([ $active[] | .value.option ] | unique)                as $distinct
    | [ $entries[]
        | select(.value.abstain == true and ((.value.rationale // "") | length > 0))
        | ({ participant: .key, rationale: .value.rationale }
           + (if (.value.option // null) != null then { option: .value.option } else {} end)) ] as $dissent
    | if $voted < $quorum then
        ( .status = "abandoned" | .closed_at = $now
          | { record: ., resolution: {
                resolved: false, status: "abandoned",
                reason: "quorum not met", quorum_required: $quorum, quorum_voted: $voted } } )
      elif ($distinct | length) == 1 then
        ( .status = "resolved" | .outcome = $distinct[0] | .closed_at = $now
          | (if ($dissent | length) > 0 then .dissent = $dissent else . end)
          | { record: ., resolution: (
                { resolved: true, status: "resolved", concord: true,
                  outcome: $distinct[0], quorum_required: $quorum, quorum_voted: $voted,
                  dissent: $dissent }
                + (if .effect == "binding" then
                     { binding_task: {
                         note: "binding outcome — council RECORDS, does not execute; emit a bwoc task to carry this out",
                         suggested_task: ("Carry out council decision " + .decision_id + ": " + .outcome) } }
                   else {} end) ) } )
      else
        ( .status = "discussing"
          | { record: ., resolution: {
                resolved: false, status: "discussing", concord: false,
                reason: (if ($distinct | length) == 0
                           then "no concord — all voters abstained; another round needed"
                           else "no concord — active votes split across options; another round needed" end),
                quorum_required: $quorum, quorum_voted: $voted, options_chosen: $distinct } } )
      end')"

  newrec="$(printf '%s' "$result" | jq '.record')"
  printf '%s\n' "$newrec" | write_record "$id"
  printf '%s' "$result" | jq '.'
}

cmd_list() {
  require_jq
  if [[ ! -d "$RECORDS_DIR" ]]; then printf '%s\n' '[]'; return; fi
  local -a files=( "$RECORDS_DIR"/*.json )
  if [[ ! -e "${files[0]}" ]]; then printf '%s\n' '[]'; return; fi
  local f
  for f in "${files[@]}"; do
    jq -e . "$f" >/dev/null 2>&1 || continue   # skip malformed; list never dies
    jq -c '{
      decision_id, status,
      question:     (.question // null),
      effect:       (.effect // null),
      options:      .options,
      participants: (.participants | length),
      rounds:       (.rounds | length),
      votes:        (.votes | length),
      outcome:      (.outcome // null),
      opened_at,
      closed_at:    (.closed_at // null)
    }' "$f"
  done | jq -s '.'
}

cmd_show() {
  require_jq
  local id; id="$(sj decision_id)"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --decision-id) id="${2:-}"; shift 2 || die "show: --decision-id needs a value" 2 ;;
      -* ) die "show: unknown flag '$1' (expected --decision-id)" 2 ;;
      * )  id="$1"; shift ;;
    esac
  done
  [[ -n "$id" ]] || die "show: --decision-id <id> is required (or: show <id>)" 2
  load_record "$id" | jq '.'
}

# ── dispatch ────────────────────────────────────────────────────────────────────

main() {
  local verb=""
  if [[ $# -gt 0 ]]; then verb="$1"; shift; else verb="${BWOC_COUNCIL_OPERATION:-}"; fi
  read_stdin_request

  case "$verb" in
    propose) cmd_propose "$@" ;;
    discuss) cmd_discuss "$@" ;;
    vote)    cmd_vote "$@" ;;
    resolve) cmd_resolve "$@" ;;
    list)    cmd_list "$@" ;;
    show)    cmd_show "$@" ;;
    "")  die "usage: protocol.sh {propose|discuss|vote|resolve|list|show} [flags]  (or set BWOC_COUNCIL_OPERATION)" 2 ;;
    *)   die "unknown verb '$verb' (expected propose|discuss|vote|resolve|list|show)" 2 ;;
  esac
}

# Dispatch only when executed directly; sourcing imports the helpers cleanly.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
