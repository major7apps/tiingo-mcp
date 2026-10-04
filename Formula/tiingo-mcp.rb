class TiingoMcp < Formula
  desc "Tiingo MCP server for stock market data, forex, crypto, news, and fundamentals"
  homepage "https://github.com/major7apps/tiingo-mcp"
  version "2.2.0"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.2.0/tiingo-mcp-aarch64-apple-darwin.tar.xz"
      sha256 "213f08b30381207cf56b4641424b8971316b80e66c1a1495c708cd22858d726e"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.2.0/tiingo-mcp-x86_64-apple-darwin.tar.xz"
      sha256 "a061e99c49b0ea1f3bf5ce2a9c3c7a7c47776c07d5333b9a838147e87ef02c8e"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.2.0/tiingo-mcp-aarch64-unknown-linux-musl.tar.xz"
      sha256 "38aa80e704fccf62c433aaebd19c8e0996dc82be2d744757bdfcbc0ae47f3eef"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.2.0/tiingo-mcp-x86_64-unknown-linux-musl.tar.xz"
      sha256 "11a78f04979dcbe4d0dd9ef75f62a67eba92b43c905546f43b8cc6c0751b29ea"
    end
  end
  license "MIT"

  BINARY_ALIASES = {
    "aarch64-apple-darwin": {},
    "aarch64-unknown-linux-gnu": {},
    "aarch64-unknown-linux-musl-dynamic": {},
    "aarch64-unknown-linux-musl-static": {},
    "x86_64-apple-darwin": {},
    "x86_64-pc-windows-gnu": {},
    "x86_64-unknown-linux-gnu": {},
    "x86_64-unknown-linux-musl-dynamic": {},
    "x86_64-unknown-linux-musl-static": {}
  }

  def target_triple
    cpu = Hardware::CPU.arm? ? "aarch64" : "x86_64"
    os = OS.mac? ? "apple-darwin" : "unknown-linux-gnu"

    "#{cpu}-#{os}"
  end

  def install_binary_aliases!
    BINARY_ALIASES[target_triple.to_sym].each do |source, dests|
      dests.each do |dest|
        bin.install_symlink bin/source.to_s => dest
      end
    end
  end

  def install
    if OS.mac? && Hardware::CPU.arm?
      bin.install "tiingo-mcp"
    end
    if OS.mac? && Hardware::CPU.intel?
      bin.install "tiingo-mcp"
    end
    if OS.linux? && Hardware::CPU.arm?
      bin.install "tiingo-mcp"
    end
    if OS.linux? && Hardware::CPU.intel?
      bin.install "tiingo-mcp"
    end

    install_binary_aliases!

    # Homebrew will automatically install these, so we don't need to do that
    doc_files = Dir["README.*", "readme.*", "LICENSE", "LICENSE.*", "CHANGELOG.*"]
    leftover_contents = Dir["*"] - doc_files

    # Install any leftover files in pkgshare; these are probably config or
    # sample files.
    pkgshare.install(*leftover_contents) unless leftover_contents.empty?
  end
end
