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
  version "2.16.1"
  license "MIT"

  # Per-platform binary download. release.yml builds 4 unix targets;
  # Windows ships as a .zip and is not consumed by Homebrew (no brew on
  # Windows). Linux ARM coverage exists because GitHub now offers free
  # ubuntu-24.04-arm runners.
  on_macos do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.31-0/bwoc-v2026.5.31-0-aarch64-apple-darwin.tar.gz"
      sha256 "63c5235554251e0eca9f13338f65230512251fba8c3d4ab9cfc8f36581caa36c"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.31-0/bwoc-v2026.5.31-0-x86_64-apple-darwin.tar.gz"
      sha256 "8d96f8a733e8e07060627801d70358c7d6a758a31f00fde3c701f3524f475f09"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.31-0/bwoc-v2026.5.31-0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "883e77bf9a2140f319cd4a2281d332a2d4d149d1893e7084c4217633d56d341c"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.31-0/bwoc-v2026.5.31-0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b9dc1fc14c7990b75a0b48ffccbbbdebc35b55a05a4e797a76474555cdf52c97"
    end
  end

  def install
    # The release tarball expands to a single subdirectory named
    # `bwoc-v<tag>-<target>/` containing the two binaries plus README/LICENSE/CHANGELOG.
    # Homebrew chdir's into single-rooted tarballs, so the files are visible at cwd.
    bin.install "bwoc"
    bin.install "bwoc-agent"
    # Ship the docs bundle into the formula's prefix for `brew home`/`brew info`.
    prefix.install "README.md" if File.exist?("README.md")
    prefix.install "LICENSE"   if File.exist?("LICENSE")
    prefix.install "CHANGELOG.md" if File.exist?("CHANGELOG.md")
  end

  test do
    # Both binaries should respond to --version. The CLI returns the Cargo
    # SemVer (not the CalVer tag), so we assert presence of the major digit
    # instead of pinning to a literal value the formula would have to track.
    assert_match "bwoc", shell_output("#{bin}/bwoc --version 2>&1")
    assert_match "bwoc-agent", shell_output("#{bin}/bwoc-agent --version 2>&1")
  end
end
