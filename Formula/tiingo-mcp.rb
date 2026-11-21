class TiingoMcp < Formula
  desc "Tiingo MCP server for stock market data, forex, crypto, news, and fundamentals"
  homepage "https://github.com/major7apps/tiingo-mcp"
  version "2.3.3"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.3/tiingo-mcp-aarch64-apple-darwin.tar.xz"
      sha256 "22b1f72fe2eb978ea5f0dd3357463331bcbac40e2fd2a2a2153ffd7bf0566da5"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.3/tiingo-mcp-x86_64-apple-darwin.tar.xz"
      sha256 "597e9455a89ae61b14cf05124a54057959f87ad131be417d3e7c947ff007f145"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.3/tiingo-mcp-aarch64-unknown-linux-musl.tar.xz"
      sha256 "6845b194f3091e546796b6e88305774c41f69bab1254fd46e79091f64861dba3"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.3/tiingo-mcp-x86_64-unknown-linux-musl.tar.xz"
      sha256 "68fef07f59a7be9384a03094ca4c2d9b11a6df9da9c69c4058f4aefbbcd68682"
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
