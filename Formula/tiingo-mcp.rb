class TiingoMcp < Formula
  desc "Tiingo MCP server for stock market data, forex, crypto, news, and fundamentals"
  homepage "https://github.com/major7apps/tiingo-mcp"
  version "2.3.2"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.2/tiingo-mcp-aarch64-apple-darwin.tar.xz"
      sha256 "80e26a02f7831ad2ac259355047b227dd0b6456087f73a585535281f9dfca2ad"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.2/tiingo-mcp-x86_64-apple-darwin.tar.xz"
      sha256 "0ccff40390e534526c210d36c52a566f2efe8fc30d3cfbd97828080b6938d94e"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.2/tiingo-mcp-aarch64-unknown-linux-musl.tar.xz"
      sha256 "09d9627da8e628668dd5d1d087a55bd44dfe31de46320e43a13fc9ba0559a5d7"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.2/tiingo-mcp-x86_64-unknown-linux-musl.tar.xz"
      sha256 "f8cdf7d918e8031787ea32b864acbbbeff52aeb93e5fbf43ceed6cbf664a6f71"
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
