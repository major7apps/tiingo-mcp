# tiingo-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/major7apps/tiingo-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/major7apps/tiingo-mcp/actions/workflows/ci.yml)

A native [Model Context Protocol](https://modelcontextprotocol.io) server for the [Tiingo](https://www.tiingo.com) financial data API. It exposes stocks, forex, crypto, news, fundamentals, and corporate actions through 17 tools, three fixed resources, one resource template, and five prompts.

The server uses MCP over stdio. Streamable HTTP and WebSocket transports are not part of the 2.0 release.

## Installation

### Release installer

On macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/major7apps/tiingo-mcp/releases/latest/download/tiingo-mcp-installer.sh | sh
```

On Windows PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/major7apps/tiingo-mcp/releases/latest/download/tiingo-mcp-installer.ps1 | iex"
```

The installers place `tiingo-mcp` in Cargo's binary directory. Ensure that directory is on `PATH` so MCP clients can use the command by name.

### Cargo

With Rust 1.88 or newer installed:

```bash
cargo install tiingo-mcp --locked
```

### MCPB desktop bundle

Target-specific bundles are named `tiingo-mcp-<target>.mcpb`. Download a published bundle for your system, open it with an MCPB-compatible desktop host's extension installer, and enter your Tiingo API key when prompted. The bundle carries the binary and MCP configuration, so it does not require a separate executable path.

Supported release targets:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-musl`
- `x86_64-unknown-linux-musl`
- `x86_64-pc-windows-msvc`

## MCP configuration

Create an API key at [api.tiingo.com](https://api.tiingo.com). Endpoint access depends on the capabilities enabled for that key.

Add the server to your MCP client's configuration:

```json
{
  "mcpServers": {
    "tiingo": {
      "command": "tiingo-mcp",
      "args": [],
      "env": { "TIINGO_API_KEY": "your-api-key-here" }
    }
  }
}
```

For Claude Code, the equivalent command is:

```bash
claude mcp add tiingo -- tiingo-mcp
```

You can also launch the server directly:

```bash
TIINGO_API_KEY=your-api-key-here tiingo-mcp
```

`tiingo-mcp` starts the stdio MCP server by default, writes protocol messages only to stdout, and sends diagnostics to stderr. It exits cleanly when stdin closes.

## Tools

### Stocks (EOD)

| Tool | Description |
|------|-------------|
| `get_stock_metadata` | Ticker information, including name, exchange, description, and date range |
| `get_stock_prices` | Historical EOD OHLCV, adjustment, dividend, and split data |

### Real-time and intraday (IEX)

| Tool | Description |
|------|-------------|
| `get_realtime_price` | Current IEX top-of-book quote |
| `get_intraday_prices` | Intraday prices at supported resample frequencies |

### Forex

| Tool | Description |
|------|-------------|
| `get_forex_quote` | Current top-of-book rate |
| `get_forex_prices` | Historical forex prices |

### Crypto

| Tool | Description |
|------|-------------|
| `get_crypto_quote` | Current prices for one or more crypto tickers |
| `get_crypto_prices` | Historical crypto prices |
| `get_crypto_metadata` | Ticker metadata and supported exchanges |

### News

| Tool | Description |
|------|-------------|
| `get_news` | Search financial articles by ticker, tag, source, or date |

### Fundamentals

| Tool | Description |
|------|-------------|
| `get_fundamentals_definitions` | Tiingo fundamental metric definitions |
| `get_financial_statements` | Income statements, balance sheets, and cash flow statements |
| `get_daily_fundamentals` | Daily market and valuation metrics |
| `get_company_meta` | Company sector, industry, and location metadata |

### Corporate actions

| Tool | Description |
|------|-------------|
| `get_dividends` | Dividend and distribution history |
| `get_dividend_yield` | Dividend yield history |
| `get_splits` | Stock split history |

## Resources

The server exposes static reference data without making Tiingo API calls.

| Resource | Description |
|----------|-------------|
| `tiingo://capabilities` | Server capabilities and source-dated entitlement guidance |
| `tiingo://fundamentals/definitions` | Curated reference for common fundamental metrics |
| `tiingo://guide/date-formats` | Date formats, resample frequencies, sort options, and parameters |
| `tiingo://guide/{asset_class}` | Guide template for stocks, forex, crypto, news, fundamentals, and corporate actions |

## Prompts

| Prompt | Arguments | Description |
|--------|-----------|-------------|
| `analyze-stock` | `ticker`, `include_news` | Comprehensive single-stock analysis |
| `compare-stocks` | `ticker1`, `ticker2`, `period` | Side-by-side stock comparison |
| `crypto-market-overview` | `tickers` | Crypto market snapshot with seven-day trends |
| `earnings-report-analysis` | `ticker`, `earnings_date` | Earnings report, price-reaction, and news workflow |
| `forex-pair-analysis` | `pair`, `period` | Currency-pair trend and volatility workflow |

## Results and errors

Successful tool calls return both a JSON text content block for older clients and MCP structured content. Recoverable failures are returned as MCP tool errors with a concise, sanitized JSON text block. The client retries only safe transient failures, limits responses to 8 MiB, and never exposes the API key or authorization header in client-visible errors.

## Development

```bash
git clone https://github.com/major7apps/tiingo-mcp.git
cd tiingo-mcp

cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
cargo deny check
```

The test suite uses local mock servers and does not need a Tiingo key. The ignored live-smoke test requires `TIINGO_API_KEY` and consumes API quota.

## License

[MIT](LICENSE)

## Links

- [Tiingo API documentation](https://www.tiingo.com/documentation/general/overview)
- [Model Context Protocol](https://modelcontextprotocol.io)
- [RMCP](https://github.com/modelcontextprotocol/rust-sdk)
- [Source repository](https://github.com/major7apps/tiingo-mcp)
