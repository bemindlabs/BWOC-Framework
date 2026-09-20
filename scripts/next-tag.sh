#!/usr/bin/env bash
# next-tag.sh — print the CalVer tag for a release cut right now.
#
# The scheme is `v<YYYY>.<M>.<D>-<patch>` with no zero padding on month/day
# (see CONTRIBUTING.md). The patch counts releases cut on the same day, so the
# next one is "today's date, first free patch".
#
# Why a script: 3.5.0 was tagged `v2026.9.21-0` on 2026-09-20 because the date
# was typed from memory rather than read from the clock. A tag that runs ahead
# of the calendar cannot be corrected afterwards — the assets are published
# under it and Homebrew resolves its version from it, so rolling back to the
# real date would sort *below* the release already shipped.
#
# Usage:
#   scripts/next-tag.sh          # the tag to cut now
#   scripts/next-tag.sh --check v2026.9.21-1
#                                # exit 0 when that tag's date is today's
#                                # (±1 day, for a cut across a UTC boundary)
#
# Dates come from the local clock, matching how every release so far was dated.
set -euo pipefail

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
}

today() { date '+%Y.%-m.%-d'; }

# All patch numbers already used for a given `<YYYY>.<M>.<D>`, on the remote as
# well as locally: a tag someone else pushed still occupies its number.
used_patches() {
  local date_part="$1"
  {
    git tag --list "v${date_part}-*"
    git ls-remote --tags origin "v${date_part}-*" 2>/dev/null | awk '{print $2}' |
      sed 's|refs/tags/||; s|\^{}$||'
  } | sed "s|^v${date_part}-||" | grep -E '^[0-9]+$' | sort -n | uniq
}

# The newest `<YYYY>.<M>.<D>` any tag already uses, as a sortable date.
latest_tag_date() {
  {
    git tag --list 'v[0-9][0-9][0-9][0-9].*'
    git ls-remote --tags origin 'v[0-9][0-9][0-9][0-9].*' 2>/dev/null |
      awk '{print $2}' | sed 's|refs/tags/||; s|\^{}$||'
  } | sed 's|^v||; s|-.*$||' |
    awk -F. 'NF==3 {printf "%04d-%02d-%02d\n", $1, $2, $3}' | sort | tail -1
}

next_tag() {
  local date_part patch latest
  date_part="$(today)"
  # A published tag dated ahead of today (it has happened) cannot be undone:
  # its assets are live and Homebrew derives the version from it, so a tag with
  # an earlier date would sort *below* the release already shipped. Stay on the
  # newest date in use until the calendar catches up.
  latest="$(latest_tag_date)"
  if [[ -n "$latest" ]] && [[ "$(printf '%s\n' "$latest" "$(date '+%Y-%m-%d')" | sort | tail -1)" == "$latest" ]] &&
    [[ "$latest" != "$(date '+%Y-%m-%d')" ]]; then
    echo "next-tag: newest tag is dated $latest, ahead of today — staying on it so the tag keeps sorting upward." >&2
    date_part="$(echo "$latest" | awk -F- '{printf "%d.%d.%d", $1, $2, $3}')"
  fi
  patch=0
  while used_patches "$date_part" | grep -qx "$patch"; do
    patch=$((patch + 1))
  done
  echo "v${date_part}-${patch}"
}

# Days between two `YYYY-MM-DD` dates, absolute.
days_apart() {
  local a b
  a=$(date -d "$1" '+%s' 2>/dev/null || date -j -f '%Y-%m-%d' "$1" '+%s')
  b=$(date -d "$2" '+%s' 2>/dev/null || date -j -f '%Y-%m-%d' "$2" '+%s')
  echo $(((a > b ? a - b : b - a) / 86400))
}

check_tag() {
  local tag="$1" stamp y m d
  # The whole shape, not just "three dots": `v2026.9.21-foo` and
  # `v2026.9.21-1-extra` must be refused, not silently mis-parsed.
  if [[ ! "$tag" =~ ^v([0-9]{4})\.([0-9]{1,2})\.([0-9]{1,2})-([0-9]+)$ ]]; then
    echo "next-tag: '$tag' is not v<YYYY>.<M>.<D>-<patch>" >&2
    return 2
  fi
  stamp="${tag#v}"
  stamp="${stamp%-*}"
  IFS=. read -r y m d <<<"$stamp"
  if ((m < 1 || m > 12 || d < 1 || d > 31)); then
    echo "next-tag: '$tag' has no such date ($y-$m-$d)" >&2
    return 2
  fi
  local tag_date now_date apart
  tag_date=$(printf '%04d-%02d-%02d' "$y" "$m" "$d")
  now_date=$(date '+%Y-%m-%d')
  apart=$(days_apart "$tag_date" "$now_date")
  if ((apart > 1)); then
    echo "next-tag: '$tag' is dated $tag_date but today is $now_date ($apart days apart)." >&2
    echo "next-tag: cut it as $(next_tag) instead." >&2
    return 1
  fi
  echo "$tag is dated $tag_date; today is $now_date — ok."
}

case "${1-}" in
"") next_tag ;;
--check)
  [[ -n "${2-}" ]] || {
    usage >&2
    exit 2
  }
  check_tag "$2"
  ;;
-h | --help) usage ;;
*)
  usage >&2
  exit 2
  ;;
esac
