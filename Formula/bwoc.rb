# Homebrew formula for BWOC — backend-neutral spec + Rust runtime for AI coding agents.
#
# Tap install:
#
#   brew tap bemindlabs/bwoc https://github.com/bemindlabs/BWOC-Framework
#   brew install bwoc
#
# Updating: when a new CalVer release lands, bump `version`, the four `url`
# lines (tag fragment) and their `sha256` to match the new release's
# `.sha256` sidecars. Each release.yml run produces sidecars at
#
#   https://github.com/bemindlabs/BWOC-Framework/releases/download/<tag>/bwoc-<tag>-<target>.tar.gz.sha256
#
# The first 64 hex chars of each file is the sha256 to paste below.

class Bwoc < Formula
  desc "BWOC framework — backend-neutral spec + Rust runtime for AI coding agents"
  homepage "https://github.com/bemindlabs/BWOC-Framework"
  version "2026.9.24.0"
  license "MIT"

  # Per-platform binary download. release.yml builds 4 unix targets;
  # Windows ships as a .zip and is not consumed by Homebrew (no brew on
  # Windows). Linux ARM coverage exists because GitHub now offers free
  # ubuntu-24.04-arm runners.
  on_macos do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-0/bwoc-v2026.9.24-0-aarch64-apple-darwin.tar.gz"
      sha256 "76ea9e48fe5a683a3945223b49247cc636b9b5421f9e9c2f680887eb04d081c2"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-0/bwoc-v2026.9.24-0-x86_64-apple-darwin.tar.gz"
      sha256 "72a810081104e690ed27f9c7dee058b7974b30822e5653d9cf35fcce16ff281a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-0/bwoc-v2026.9.24-0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "38944c9a456851ebbdd499db9275ea57f925fe31a4ba971887817481edd7be3d"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-0/bwoc-v2026.9.24-0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "945dee48d6093a8165dece07abd9f6c07d12a5b726cd89f83e5f4f9a83a027e4"
    end
  end

  def install
    # The release tarball expands to a single subdirectory named
    # `bwoc-v<tag>-<target>/` containing the three binaries plus README/LICENSE/CHANGELOG.
    # Homebrew chdir's into single-rooted tarballs, so the files are visible at cwd.
    bin.install "bwoc"
    bin.install "bwoc-agent"
    # `bwoc-harness` runs the agentic loop, and `bwoc` resolves it as a sibling
    # of itself — installing it into the same `bin` is what makes the harness
    # paths (chat --tui, eval, --headless, --lead, --task) work on a brew
    # install. Archives carry it from v2026.8.20-1 onward (issue #460).
    bin.install "bwoc-harness"
    # Ship the docs bundle into the formula's prefix for `brew home`/`brew info`.
    prefix.install "README.md" if File.exist?("README.md")
    prefix.install "LICENSE"   if File.exist?("LICENSE")
    prefix.install "CHANGELOG.md" if File.exist?("CHANGELOG.md")
  end

  test do
    # All three binaries should respond to --version. Each prints the Cargo
    # SemVer, not the CalVer tag this formula's `version` carries, so assert
    # only that the binary names itself — pinning a literal version here would
    # mean editing the test on every release.
    assert_match "bwoc", shell_output("#{bin}/bwoc --version 2>&1")
    assert_match "bwoc-agent", shell_output("#{bin}/bwoc-agent --version 2>&1")
    # Harness included deliberately: its absence was invisible for many
    # releases (#460) precisely because nothing asserted it. `brew test` now
    # fails rather than shipping a bwoc that cannot spawn its own loop.
    assert_match "bwoc-harness", shell_output("#{bin}/bwoc-harness --version 2>&1")
  end
end
