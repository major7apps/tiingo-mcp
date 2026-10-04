class TiingoMcp < Formula
  desc "MCP server for Tiingo stock market and financial data"
  homepage "https://github.com/major7apps/tiingo-mcp"
  url "https://github.com/major7apps/tiingo-mcp/archive/refs/tags/v2.2.0.tar.gz"
  sha256 "119104f796107956ed4fce81e9e1e5d003c76020f61a65070426ab5618977e0f"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    require "json"
    require "open3"
    require "timeout"

    Open3.popen3(
      { "TIINGO_API_KEY" => nil, "RUST_LOG" => "error" },
      (bin/"tiingo-mcp").to_s,
    ) do |stdin, stdout, stderr, process|
      Timeout.timeout(10) do
        stdin.puts JSON.generate(
          jsonrpc: "2.0",
          id:      1,
          method:  "initialize",
          params:  {
            protocolVersion: "2025-11-25",
            capabilities:    {},
            clientInfo:      { name: "homebrew-test", version: "1.0.0" },
          },
        )
        stdin.flush
        initialized = JSON.parse(stdout.readline)
        assert_equal 1, initialized.fetch("id")
        assert_equal "2025-11-25", initialized.dig("result", "protocolVersion")
        assert_equal version.to_s, initialized.dig("result", "serverInfo", "version")

        stdin.puts JSON.generate(jsonrpc: "2.0", method: "notifications/initialized")
        stdin.puts JSON.generate(jsonrpc: "2.0", id: 2, method: "tools/list", params: {})
        stdin.flush
        response = JSON.parse(stdout.readline)
        assert_equal 2, response.fetch("id")
        tools = response.fetch("result").fetch("tools").map { |tool| tool.fetch("name") }
        assert_equal 38, tools.length
        assert_includes tools, "get_stock_prices"
        assert_includes tools, "start_market_data_subscription"

        stdin.close
        assert_predicate process.value, :success?, stderr.read
      end
    ensure
      stdin.close unless stdin.closed?
      if process.alive?
        begin
          Process.kill("KILL", process.pid)
        rescue Errno::ESRCH
          nil
        end
      end
    end
  end
end
