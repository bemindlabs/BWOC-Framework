#!/usr/bin/env bash
# bilingual-reminder.sh — PostToolUse Write|Edit hook
#
# Reminds the model to keep translation pairs in sync. English is canonical;
# translations live as `<NAME>.<lang>.md` (lowercase BCP 47 / ISO 639-1 — see
# NAMING.en.md). Despite the historical name, this is **language-agnostic**:
# it works for any `docs/<lang>/` (en/th today, ja/zh/… in future), not just TH.
#
# Two patterns:
#   1. */docs/<lang>/<NAME>.<lang>.md — template + framework spec docs.
#        - editing a NON-canonical translation → remind about the EN canonical.
#        - editing the EN canonical → remind about every translation that
#          ALREADY exists (it never nags to create a translation in some
#          unknown language; parity is only enforced where it already holds).
#   2. <repo-root>/FILENAME.md ↔ <repo-root>/FILENAME.<lang>.md — root metadata
#      (e.g. VISION.md ↔ VISION.th.md). Same rule: translation→canonical always;
#      canonical→translations only for those that already exist.
#
# Pure nudge — non-blocking, no exit-2. Output is JSON additionalContext.

set -euo pipefail

f=$(jq -r '.tool_input.file_path // empty')
[[ -z "$f" ]] && exit 0

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
rel="${f#"$repo_root"/}"

# Skip files outside the repo.
case "$rel" in
  /*) exit 0 ;;
esac

# Emit the reminder JSON and exit.
emit() { jq -n --arg m "$1" '{hookSpecificOutput:{hookEventName:"PostToolUse",additionalContext:$m}}'; exit 0; }

# --- Pattern 1: docs/<lang>/<NAME>.<lang>.md ---------------------------------
if [[ "$f" =~ /docs/([a-z]{2,3})/(.+)\.([a-z]{2,3})\.md$ ]]; then
  dir_lang="${BASH_REMATCH[1]}"
  name="${BASH_REMATCH[2]}"
  file_lang="${BASH_REMATCH[3]}"
  # Only act when the directory language matches the filename suffix (the
  # canonical docs/<lang>/<NAME>.<lang>.md shape).
  if [[ "$dir_lang" == "$file_lang" ]]; then
    docs_dir="${f%/"$dir_lang"/*}"          # …/docs
    if [[ "$dir_lang" == "en" ]]; then
      # Canonical edit → list existing translations to update.
      others=""
      for cand in "$docs_dir"/*/"$name".*.md; do
        [[ -f "$cand" ]] || continue
        [[ "$cand" == "$f" ]] && continue
        others+="${others:+, }$cand"
      done
      [[ -n "$others" ]] && emit "translation parity: you edited the EN canonical — also update: $others"
      exit 0
    else
      # Translation edit → point at the EN canonical (required; create if absent).
      canonical="$docs_dir/en/$name.en.md"
      if [[ -f "$canonical" ]]; then
        emit "translation parity: also keep the EN canonical in sync: $canonical"
      else
        emit "translation parity: the EN canonical is MISSING — create $canonical (English is the source of truth)"
      fi
    fi
  fi
fi

# --- Pattern 2: root-level FILENAME.md ↔ FILENAME.<lang>.md -------------------
# Only files directly at the repo root (no subdirectory in `rel`).
if [[ "$rel" != */* ]]; then
  if [[ "$rel" =~ ^(.+)\.([a-z]{2,3})\.md$ ]]; then
    # A translation like VISION.th.md → remind about the canonical.
    name="${BASH_REMATCH[1]}"
    canonical="$repo_root/$name.md"
    if [[ -f "$canonical" ]]; then
      emit "translation parity: also keep the canonical in sync: $name.md"
    else
      emit "translation parity: canonical $name.md is MISSING — create it (English is canonical)"
    fi
  elif [[ "$rel" =~ ^(.+)\.md$ ]]; then
    # A canonical like VISION.md → remind about existing translations only.
    name="${BASH_REMATCH[1]}"
    others=""
    for cand in "$repo_root/$name".*.md; do
      [[ -f "$cand" ]] || continue
      others+="${others:+, }$(basename "$cand")"
    done
    [[ -n "$others" ]] && emit "translation parity: you edited the canonical — also update: $others"
  fi
fi

exit 0
