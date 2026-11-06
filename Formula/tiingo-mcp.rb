class TiingoMcp < Formula
  desc "Tiingo MCP server for stock market data, forex, crypto, news, and fundamentals"
  homepage "https://github.com/major7apps/tiingo-mcp"
  version "2.3.1"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.1/tiingo-mcp-aarch64-apple-darwin.tar.xz"
      sha256 "cad3e8eb8d2627967c29108adaa298872e584a15358f4cd5b235821e5f347ab0"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.1/tiingo-mcp-x86_64-apple-darwin.tar.xz"
      sha256 "7ffc66609bad19d99a9a65cab6906a0ba01b5814d59c4c0868fec61c579d63be"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.1/tiingo-mcp-aarch64-unknown-linux-musl.tar.xz"
      sha256 "a0c8b149d2463f7acd9136c753b7e949c561076cc49135189d0c110bc3a68529"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.1/tiingo-mcp-x86_64-unknown-linux-musl.tar.xz"
      sha256 "01744659138e5837c498166848d7cfc0bd00643fa9ec59ba654688f2f08ce674"
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
