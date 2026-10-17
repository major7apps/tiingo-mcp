class TiingoMcp < Formula
  desc "Tiingo MCP server for stock market data, forex, crypto, news, and fundamentals"
  homepage "https://github.com/major7apps/tiingo-mcp"
  version "2.3.0"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.0/tiingo-mcp-aarch64-apple-darwin.tar.xz"
      sha256 "84905b09cc1897be38d08e074d5a4210d51bd0dd19c2f8b58dd40d2b5190692b"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.0/tiingo-mcp-x86_64-apple-darwin.tar.xz"
      sha256 "e1d99d21689b2d753f966ac35e799ab4ff3f83f807157e689df5f328b8ac97f0"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.0/tiingo-mcp-aarch64-unknown-linux-musl.tar.xz"
      sha256 "80884ebb848b1738037db87137d3454e4657210b95a3ef0d83a417fbcef1ff95"
    end
    if Hardware::CPU.intel?
      url "https://github.com/major7apps/tiingo-mcp/releases/download/v2.3.0/tiingo-mcp-x86_64-unknown-linux-musl.tar.xz"
      sha256 "8b4ed4d0a91ee566af4aac31926df33af87e56986b082dd53ef5185fcce28978"
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
