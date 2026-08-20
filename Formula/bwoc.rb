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
  version "2026.8.20.1"
  license "MIT"

  # Per-platform binary download. release.yml builds 4 unix targets;
  # Windows ships as a .zip and is not consumed by Homebrew (no brew on
  # Windows). Linux ARM coverage exists because GitHub now offers free
  # ubuntu-24.04-arm runners.
  on_macos do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.8.20-1/bwoc-v2026.8.20-1-aarch64-apple-darwin.tar.gz"
      sha256 "210fa80b0246f960df369cad689efb530b2255ab6b06383d587765cea604b280"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.8.20-1/bwoc-v2026.8.20-1-x86_64-apple-darwin.tar.gz"
      sha256 "a976288b86594b2cb8addd4225222a983e9c158def33442de0d49a6fad2850d4"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.8.20-1/bwoc-v2026.8.20-1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "4d41373e9a9f970b1e109b0a1f95f34db31710228a1ca4ff378b462477fdf8e1"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.8.20-1/bwoc-v2026.8.20-1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "f80963210f82b0060041768e83a642f08086998906cb88f5f9a95473acdcb826"
    end
  end

  def install
    # The release tarball expands to a single subdirectory named
    # `bwoc-v<tag>-<target>/` containing the three binaries plus README/LICENSE/CHANGELOG.
    # Homebrew chdir's into single-rooted tarballs, so the files are visible at cwd.
    bin.install "bwoc"
    bin.install "bwoc-agent"
    # `bwoc-harness` runs the agentic loop, and `bwoc` resolves it as a sibling
    # of itself — installing it into the same `bin` is what makes `bwoc chat
    # --tui`, `bwoc eval`, `--headless`, `--lead` and `--task` work on a brew
    # install. Archives carry it from v2026.8.20-1 onward (issue #460).
    bin.install "bwoc-harness"
    # Ship the docs bundle into the formula's prefix for `brew home`/`brew info`.
    prefix.install "README.md" if File.exist?("README.md")
    prefix.install "LICENSE"   if File.exist?("LICENSE")
    prefix.install "CHANGELOG.md" if File.exist?("CHANGELOG.md")
  end

  test do
    # All three binaries should respond to --version. The CLI returns the Cargo
    # SemVer (not the CalVer tag), so we assert presence of the major digit
    # instead of pinning to a literal value the formula would have to track.
    assert_match "bwoc", shell_output("#{bin}/bwoc --version 2>&1")
    assert_match "bwoc-agent", shell_output("#{bin}/bwoc-agent --version 2>&1")
    # Harness included deliberately: its absence was invisible for many
    # releases (#460) precisely because nothing asserted it. `brew test` now
    # fails rather than shipping a bwoc that cannot spawn its own loop.
    assert_match "bwoc-harness", shell_output("#{bin}/bwoc-harness --version 2>&1")
  end
end
