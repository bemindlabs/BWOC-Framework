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
  version "2026.9.24.1"
  license "MIT"

  # Per-platform binary download. release.yml builds 4 unix targets;
  # Windows ships as a .zip and is not consumed by Homebrew (no brew on
  # Windows). Linux ARM coverage exists because GitHub now offers free
  # ubuntu-24.04-arm runners.
  on_macos do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-1/bwoc-v2026.9.24-1-aarch64-apple-darwin.tar.gz"
      sha256 "d5673573856acfa6084a69bdbaf2eb21c1a5d079258cb9b459ff6a6abdb5e1d5"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-1/bwoc-v2026.9.24-1-x86_64-apple-darwin.tar.gz"
      sha256 "f770d88468b0ceb9798f0e39ea2238e45fe20fe1b429fbfb757f7dbdca378896"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-1/bwoc-v2026.9.24-1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "a74c3833e84dcd638e713f44fc56f7af216575ad822af8be3763bed5ad8b1bb2"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.9.24-1/bwoc-v2026.9.24-1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c17fbce2be3538ebfdbdb3e13e795c5e6014d50b73d66184af1a073125fa7851"
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
