#!/usr/bin/env bash
#
# scripts/check-deferred-fence.sh — Phase 5 t8 deferred-control fence-guard.
#
# t8 is an HONESTY gate, not a coverage gate. It did not implement the controls it
# fenced (cgroup pids.max / seccomp — tickets t9 / t11); it made their absence
# impossible to forget or fake. Both have since LANDED, so the SSOT now enumerates
# no tokens and the fence is FULLY DISCHARGED — but the guard still runs every pass
# to keep that terminal state honest (a re-introduced phantom, or a dropped
# residual, fails CI). This guard enforces in CI:
#
#   A. SSOT spellings — scripts/deferred-controls.txt enumerates the REAL kernel/
#      library spellings of each STILL-deferred control, bound to its ticket. Those
#      are the tokens the phantom-control guard greps for in live `.rs`. (Empty now
#      = discharged; a future deferral re-arms the machinery automatically.)
#   B. Phantom-control guard — a deferred token appearing in live (comment- and
#      test-stripped) `.rs` is "must-annotate", not instant-fail. Annotate the
#      line with `// DEFERRED(tNN):` (matching ticket) and it passes. This covers
#      string literals too, so an honest error like "seccomp unavailable" does not
#      false-positive the guard against itself — it just has to say it's deferred.
#      (No-op while discharged: a landed control's tokens leave the SSOT, so its
#      now-live references — cgroup / pids.max — are no longer phantom.)
#   C. The honest t9 RESIDUAL is asserted to stay written in the THREAT-MODEL
#      (EN + TH lock-step): cgroup `pids.max` caps a turn WHEN a subtree is
#      delegated, else the fork guard degrades to the best-effort per-UID
#      `RLIMIT_NPROC` floor. NPROC keeps its RELATIVE/best-effort marker in live
#      code and is never treated as a hard guarantee (.expect/assert!).
#   E. Bidirectional doc-sync — every ticket in the SSOT appears in the fence
#      region AND vice-versa; the SSOT token set equals the region's token set.
#      Drift in EITHER direction fails CI. Bilingual: EN and TH regions must carry
#      the identical ticket + token sets (both empty while discharged).
#
# Exit 0 = fence intact. Non-zero = drift; the message names what and where.
#
# Implementation honesty (musāvāda): the comment/string/test stripper below is a
# pragmatic Rust-aware pass, not a full parser. It handles `//` + `/* */`
# comments (string-aware), preserves string/char literal CONTENTS (so tokens in
# strings are still detected — condition B), distinguishes `'a` lifetimes from
# `'c'` char literals, and blanks `#[cfg(test)]` mod/fn blocks by brace-match.
# It does NOT fully model raw strings with embedded quotes (`r#"..."#`); a
# deferred token hidden inside one could be missed. No such usage exists today
# and the limitation is documented rather than silently assumed away.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

SSOT="scripts/deferred-controls.txt"
EN="docs/en/THREAT-MODEL.en.md"
TH="docs/th/THREAT-MODEL.th.md"
TURN_EXEC="crates/bwoc-harness/src/turn_executor.rs"

fail=0
err()  { printf 'FENCE-GUARD FAIL: %s\n' "$1" >&2; fail=1; }
note() { printf '  fence-guard: %s\n' "$1"; }

for f in "$SSOT" "$EN" "$TH" "$TURN_EXEC"; do
  [ -f "$f" ] || { err "required file missing: $f"; }
done
[ "$fail" -eq 0 ] || exit 1

# ── parse the SSOT ──────────────────────────────────────────────────────────
# Portable to bash 3.2 (macOS) — no associative arrays. The SSOT itself is the
# token→ticket map; ticket_of() looks a token up directly.
#   non-comment line = "<ticket> <token>"
ssot_pairs="$(awk '
  /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
  { print $1, $2 }
' "$SSOT")"
ssot_tokens_sorted="$(printf '%s\n' "$ssot_pairs" | awk '{print $2}' | LC_ALL=C sort -u)"
ssot_tickets_sorted="$(printf '%s\n' "$ssot_pairs" | awk '{print $1}' | LC_ALL=C sort -u)"

# Discharged state: every Phase-5 deferred control has landed (t9 + t11), so the
# SSOT enumerates no tokens. That is the terminal SUCCESS of the fence, not a
# failure — skip the phantom-control + token-sync checks (there is nothing left to
# fence) and assert the honest residual is documented instead (condition C below).
# A NON-empty fence still runs the full machinery.
if [ -z "$ssot_tokens_sorted" ]; then
  note "SSOT enumerates no deferred tokens — the Phase 5 deferred-control fence is FULLY DISCHARGED (t9 + t11 landed). Asserting the honest residual; phantom/token-sync checks have nothing to fence."
  DISCHARGED=1
else
  DISCHARGED=0
  # Validate ticket spellings + completeness up front.
  printf '%s\n' "$ssot_pairs" | while read -r tk tok; do
    case "$tk"  in t[0-9]*) ;; *) echo "BADTICKET $tk" ;; esac
    [ -z "$tok" ] && echo "NOTOKEN $tk"
  done | grep -q . && { err "malformed SSOT line(s) (ticket must be t<NN>, token required)"; }
fi

ticket_of() { awk -v t="$1" '$2==t {print $1; exit}' <<EOF
$ssot_pairs
EOF
}

# ── extract the fenced region of a markdown doc ─────────────────────────────
# Everything strictly between DEFERRED-FENCE:BEGIN and DEFERRED-FENCE:END.
fence_region() {
  awk '/DEFERRED-FENCE:BEGIN/{f=1;next} /DEFERRED-FENCE:END/{f=0} f' "$1"
}

# tickets in a region: \bt<NN>\b ; tokens in a region: every `backticked` run.
# `|| true`: a discharged fence has a token/ticket-free region, so grep finds
# nothing and exits 1 — that is the empty set, not an error (don't trip set -e).
region_tickets() { grep -oE '\bt[0-9]+\b' | LC_ALL=C sort -u || true; }
region_tokens()  { grep -oE '`[^`]+`' | tr -d '`' | LC_ALL=C sort -u || true; }

# ── E: bidirectional doc-sync, per language ─────────────────────────────────
check_doc_sync() {
  local doc="$1" lang="$2" region t_tickets t_tokens
  region="$(fence_region "$doc")"
  if [ -z "$region" ]; then
    err "$lang: no DEFERRED-FENCE:BEGIN/END region in $doc"
    return
  fi
  t_tickets="$(printf '%s\n' "$region" | region_tickets)"
  t_tokens="$(printf '%s\n' "$region" | region_tokens)"

  if [ "$t_tickets" != "$ssot_tickets_sorted" ]; then
    err "$lang ticket set drifts from SSOT (bidirectional)."
    diff <(printf '%s\n' "$ssot_tickets_sorted") <(printf '%s\n' "$t_tickets") \
      | sed 's/^/    /' >&2 || true
  fi
  if [ "$t_tokens" != "$ssot_tokens_sorted" ]; then
    err "$lang token set drifts from SSOT (table↔guard, bidirectional)."
    diff <(printf '%s\n' "$ssot_tokens_sorted") <(printf '%s\n' "$t_tokens") \
      | sed 's/^/    /' >&2 || true
  fi
}
check_doc_sync "$EN" "EN"
check_doc_sync "$TH" "TH"

# ── C: the honest t9 residual must stay written in the THREAT-MODEL (EN + TH) ──
# t9 LANDED — cgroup `pids.max` caps a turn WHERE a writable subtree is delegated.
# Its residual — when NOT delegated (the DEFAULT: dev / bare-SSH / non-delegated
# container) the fork guard degrades to the best-effort per-UID `RLIMIT_NPROC`
# floor — moved out of the now-discharged fence region into the Residuals section.
# Assert it stays spelled out in BOTH languages (lock-step), AND that the refined
# "pids.max when delegated, else best-effort" truth is present — so '✅-when-
# delegated' can never silently overwrite the 🟠 common case.
for doc in "$EN" "$TH"; do
  grep -q 'RLIMIT_NPROC' "$doc" \
    || err "$doc must keep the honest t9 residual: RLIMIT_NPROC (the per-UID fork guard when no cgroup is delegated)."
  grep -qiE 'per-uid' "$doc" \
    || err "$doc must state the t9 residual RLIMIT_NPROC is per-UID (condition C)."
  grep -q 'pids.max' "$doc" \
    || err "$doc must name the landed t9 control (cgroup pids.max)."
  grep -qiE 'delegat' "$doc" \
    || err "$doc must state pids.max enforces only when a cgroup subtree is delegated (else best-effort) — don't let ✅-when-delegated mask the 🟠 default."
done

# ── C (best-effort rule): NPROC stays best-effort/RELATIVE, never a hard guarantee
if ! grep -q 'RLIMIT_NPROC' "$TURN_EXEC"; then
  err "$TURN_EXEC no longer references RLIMIT_NPROC — fence assumptions stale."
fi
if ! grep -qi 'RELATIVE' "$TURN_EXEC" || ! grep -qi 'best-effort' "$TURN_EXEC"; then
  err "$TURN_EXEC must keep the RELATIVE / best-effort marker on RLIMIT_NPROC."
fi

# ── the Rust comment/string/test stripper (perl) ────────────────────────────
# Emits the file with comments blanked, string/char CONTENTS preserved, and
# #[cfg(test)] mod/fn blocks blanked. Line numbers are preserved 1:1.
STRIP_PL='
undef $/;
my $t = <STDIN>;
my @c = split //, $t;
my ($i,$n) = (0, scalar @c);
my ($line,$blk,$str,$chr) = (0,0,0,0);
my $o = "";
while ($i < $n) {
  my $ch = $c[$i];
  my $nx = ($i+1<$n) ? $c[$i+1] : "";
  if ($line) { if ($ch eq "\n"){$line=0;$o.=$ch}else{$o.=" "} $i++; next; }
  if ($blk)  { if ($ch eq "*" && $nx eq "/"){$blk=0;$o.="  ";$i+=2;next}
               $o .= ($ch eq "\n")?"\n":" "; $i++; next; }
  if ($str)  { $o.=$ch; if($ch eq "\\"){ $o.=$nx if $i+1<$n; $i+=2; next }
               if($ch eq "\""){$str=0} $i++; next; }
  if ($chr)  { $o.=$ch; if($ch eq "\\"){ $o.=$nx if $i+1<$n; $i+=2; next }
               if($ch eq "\x27"){$chr=0} $i++; next; }
  if ($ch eq "/" && $nx eq "/"){ $line=1; $o.="  "; $i+=2; next; }
  if ($ch eq "/" && $nx eq "*"){ $blk=1;  $o.="  "; $i+=2; next; }
  if ($ch eq "\""){ $str=1; $o.=$ch; $i++; next; }
  if ($ch eq "\x27"){
    my $nn = ($i+2<$n) ? $c[$i+2] : "";
    if ($nx eq "\\" || $nn eq "\x27"){ $chr=1; $o.=$ch; $i++; next; }
    $o.=$ch; $i++; next;   # lifetime, not a char literal
  }
  $o .= $ch; $i++;
}
# blank #[cfg(test)] mod/fn blocks by brace match (string/char aware)
while (1) {
  my $p = index($o, "#[cfg(test)]");
  last if $p < 0;
  my $b = index($o, "{", $p);
  my $attrlen = length("#[cfg(test)]");
  if ($b < 0) { substr($o,$p,$attrlen) = " " x $attrlen; next; }
  my $between = substr($o, $p, $b-$p);
  unless ($between =~ /\b(mod|fn)\b/) { substr($o,$p,$attrlen)=" " x $attrlen; next; }
  my ($depth,$j,$end,$s,$h) = (0,$b,-1,0,0);
  my $len = length($o);
  while ($j < $len) {
    my $k = substr($o,$j,1);
    if ($s){ if($k eq "\\"){$j+=2;next} if($k eq "\""){$s=0} $j++; next; }
    if ($h){ if($k eq "\\"){$j+=2;next} if($k eq "\x27"){$h=0} $j++; next; }
    if ($k eq "\""){$s=1;$j++;next}
    if ($k eq "\x27"){ my $x=substr($o,$j+1,1); my $y=substr($o,$j+2,1);
                       if($x eq "\\" || $y eq "\x27"){$h=1} $j++; next; }
    if ($k eq "{"){$depth++}
    elsif ($k eq "}"){$depth--; if($depth==0){$end=$j;last}}
    $j++;
  }
  last if $end < 0;
  my $span = substr($o,$p,$end-$p+1);
  $span =~ s/[^\n]/ /g;
  substr($o,$p,$end-$p+1) = $span;
}
print $o;
'

# ── B: phantom-control guard over live src ──────────────────────────────────
# Live src = crates/*/src/**/*.rs, minus the red-team bin (gate-only, behind the
# test-redteam feature). Integration tests (crates/*/tests) and benches are not
# under src, so they are already out.
LIVE_RS=()
while IFS= read -r f; do
  [ -n "$f" ] && LIVE_RS+=("$f")
done < <(find crates -type f -name '*.rs' -path '*/src/*' \
           ! -name 'sandbox_redteam.rs' | LC_ALL=C sort)
note "scanning ${#LIVE_RS[@]} live src files for deferred tokens"

phantom_hits=0
for file in "${LIVE_RS[@]}"; do
  stripped="$(perl -e "$STRIP_PL" < "$file")"
  # C (best-effort rule): RLIMIT_NPROC is per-UID best-effort, never an absolute
  # guarantee. Flag any live-code line that treats it as one (.expect/assert!).
  if printf '%s\n' "$stripped" | grep -E 'NPROC' | grep -qE '\.expect\(|assert!'; then
    err "RLIMIT_NPROC is treated as a hard guarantee (.expect/assert!) in live code at $file — it is best-effort per-UID, keep it conditional."
  fi
  for token in $ssot_tokens_sorted; do
    # grep fixed-string for the token in the stripped (comment/test-free) text.
    while IFS=: read -r lineno _; do
      [ -z "${lineno:-}" ] && continue
      ticket="$(ticket_of "$token")"
      # Escape hatch: `// DEFERRED(tNN):` on the same line or the line above,
      # in the ORIGINAL file, with the matching ticket.
      annot_re="// *DEFERRED($ticket):"
      same="$(sed -n "${lineno}p" "$file")"
      prev_no=$(( lineno > 1 ? lineno - 1 : 1 ))
      prev="$(sed -n "${prev_no}p" "$file")"
      if printf '%s\n%s\n' "$same" "$prev" | grep -qF "// DEFERRED($ticket):" \
         || printf '%s\n%s\n' "$same" "$prev" | grep -qE "$annot_re"; then
        continue   # acknowledged deferred placeholder — OK
      fi
      err "phantom control: deferred token '$token' ($ticket) in live code at ${file}:${lineno} without a '// DEFERRED($ticket):' annotation."
      phantom_hits=$((phantom_hits+1))
    done < <(printf '%s\n' "$stripped" | grep -nF -- "$token" || true)
  done
done
[ "$phantom_hits" -eq 0 ] && note "no unannotated deferred tokens in live code"

if [ "$fail" -ne 0 ]; then
  printf '\nfence-guard: FAILED — the deferred-control fence has drifted.\n' >&2
  exit 1
fi
printf 'fence-guard: OK — deferred-control fence intact (SSOT↔table↔guard in sync; no phantom controls).\n'
