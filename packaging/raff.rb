# Homebrew formula template for raff.
#
# The release workflow replaces the placeholders below before pushing this file
# into the Homebrew tap repository.

class Raff < Formula
  desc "Rust architecture fitness functions"
  homepage "https://github.com/liamwh/raff"
  license "MIT"
  version "__VERSION__"

  on_arm do
    url "https://github.com/liamwh/raff/releases/download/v__VERSION__/raff-aarch64-apple-darwin.tar.gz"
    sha256 "__ARM64_SHA256__"
  end

  on_intel do
    url "https://github.com/liamwh/raff/releases/download/v__VERSION__/raff-x86_64-apple-darwin.tar.gz"
    sha256 "__X86_64_SHA256__"
  end

  depends_on :macos

  def install
    bin.install "raff"
  end

  test do
    assert_match "raff", shell_output("#{bin}/raff --help")
  end
end
