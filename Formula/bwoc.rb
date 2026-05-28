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
  version "2.10.0"
  license "MIT"

  # Per-platform binary download. release.yml builds 4 unix targets;
  # Windows ships as a .zip and is not consumed by Homebrew (no brew on
  # Windows). Linux ARM coverage exists because GitHub now offers free
  # ubuntu-24.04-arm runners.
  on_macos do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.28-0/bwoc-v2026.5.28-0-aarch64-apple-darwin.tar.gz"
      sha256 "873e2acc42edba5cad60d221865c356eb271ea3d0e7300e1e019f965a5ad45f8"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.28-0/bwoc-v2026.5.28-0-x86_64-apple-darwin.tar.gz"
      sha256 "6db5953310a12508e8403f6ad50f4b59a82cd7eb1f3d5abba47929c98b55504d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.28-0/bwoc-v2026.5.28-0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "980f526c9a01316168e87f1a0ccc11969ffe5823832e21b5182c6b779f9a2906"
    end
    on_intel do
      url "https://github.com/bemindlabs/BWOC-Framework/releases/download/v2026.5.28-0/bwoc-v2026.5.28-0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "fa917abb668c3746283ce5d57aef282a081394b4f5894ad34a7695446fa10f22"
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
