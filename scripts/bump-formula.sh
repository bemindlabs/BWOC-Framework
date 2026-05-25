#!/usr/bin/env bash
# bump-formula.sh — rewrite Formula/bwoc.rb to match a published release.
#
# Called by .github/workflows/release.yml after the matrix build uploads the
# .tar.gz + .sha256 sidecars (issue #52). The bump logic lives here, not
# inline in the workflow, so it is testable locally: point it at a directory
# of sidecars for any tag and diff Formula/bwoc.rb.
#
# Usage:
#   scripts/bump-formula.sh <tag> <sidecar-dir>
#
#   <tag>          CalVer release tag, e.g. v2026.5.25-0 — the url fragment.
#   <sidecar-dir>  directory holding bwoc-<tag>-<target>.tar.gz.sha256 for the
#                  4 unix targets the formula consumes.
#
# What it rewrites in Formula/bwoc.rb:
#   - version       <- Cargo.toml [workspace.package].version (the SemVer the
#                      CLI reports; the tag is CalVer, a different scheme).
#   - 4 url lines   <- the new <tag> spliced into each download URL. The
#                      owner/repo host is read from the formula itself and
#                      preserved — release URLs need not match the CI repo
#                      ($GITHUB_REPOSITORY), so we never derive it from there.
#   - 4 sha256      <- the first hex field of each matching sidecar.
#
# Sila/Sacca: a published formula must truthfully match the released binaries,
# so a missing sidecar or a malformed sha is a hard error, not a silent skip.
set -euo pipefail

tag="${1:-}"
sidecar_dir="${2:-}"
if [ -z "$tag" ] || [ -z "$sidecar_dir" ]; then
  echo "usage: $0 <tag> <sidecar-dir>" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
formula="$repo_root/Formula/bwoc.rb"
cargo_toml="$repo_root/Cargo.toml"

[ -f "$formula" ]    || { echo "error: no formula at $formula" >&2; exit 1; }
[ -f "$cargo_toml" ] || { echo "error: no Cargo.toml at $cargo_toml" >&2; exit 1; }

# The 4 unix targets the formula consumes (the Windows .zip is not). Order
# matches the on_macos/on_linux blocks but the script keys off the target
# substring in each url line, so order here is informational.
targets=(
  aarch64-apple-darwin
  x86_64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-unknown-linux-gnu
)

# SemVer for the `version` line. Top-level `^version =` is unique to
# [workspace.package] (deps declare `name = { version = ... }`), matching
# scripts/bump-version.sh's extraction.
version="$(grep -E '^version = "[0-9]+\.[0-9]+\.[0-9]+"' "$cargo_toml" \
           | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)"
[ -n "$version" ] || { echo "error: could not read workspace version from $cargo_toml" >&2; exit 1; }

# Owner/repo prefix lives in the formula's existing download URLs — preserve
# it rather than assume the CI repo is the release host.
base="$(grep -oE 'https://github\.com/[^/]+/[^/]+/releases/download/' "$formula" \
        | head -1 | sed 's#/releases/download/$##')"
[ -n "$base" ] || { echo "error: could not read release host from $formula urls" >&2; exit 1; }

for target in "${targets[@]}"; do
  sidecar="$sidecar_dir/bwoc-${tag}-${target}.tar.gz.sha256"
  [ -f "$sidecar" ] || { echo "error: missing sidecar $sidecar" >&2; exit 1; }
  # shasum output is "<sha>  <filename>"; take the first field.
  sha="$(awk '{print $1; exit}' "$sidecar")"
  [[ "$sha" =~ ^[0-9a-f]{64}$ ]] || { echo "error: bad sha256 in $sidecar: '$sha'" >&2; exit 1; }
  url="${base}/releases/download/${tag}/bwoc-${tag}-${target}.tar.gz"

  # Rewrite the url line that names this target and the sha256 line directly
  # beneath it (the formula always pairs them). awk keeps the pairing exact;
  # `sub` preserves the line's indentation.
  awk -v t="$target" -v url="$url" -v sha="$sha" '
    index($0, "url \"") && index($0, t) {
      sub(/url ".*"/, "url \"" url "\"")
      print; expect_sha = 1; next
    }
    expect_sha && index($0, "sha256 \"") {
      sub(/sha256 ".*"/, "sha256 \"" sha "\"")
      expect_sha = 0; print; next
    }
    { print }
  ' "$formula" > "$formula.tmp"
  mv "$formula.tmp" "$formula"
done

# version line (2-space indent, distinct from any dependency version).
sed -i.bak -E "s|^(  version \").*(\")|\1${version}\2|" "$formula"
rm -f "$formula.bak"

# Verify the rewrite landed: exactly 4 urls for this tag, version set.
n="$(grep -c "/releases/download/${tag}/bwoc-${tag}-" "$formula" || true)"
[ "$n" -eq 4 ] || { echo "error: expected 4 url lines for ${tag}, found ${n}" >&2; exit 1; }
grep -q "^  version \"${version}\"" "$formula" \
  || { echo "error: version line not updated to ${version}" >&2; exit 1; }

echo "Formula bumped: version ${version}, tag ${tag} (4 targets)."
