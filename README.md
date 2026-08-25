# tiingo-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/major7apps/tiingo-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/major7apps/tiingo-mcp/actions/workflows/ci.yml)

A native [Model Context Protocol](https://modelcontextprotocol.io) server for the [Tiingo](https://www.tiingo.com) financial data API. It exposes EOD, IEX, consolidated equity, BOATS, forex, crypto, Crypto Yield, funds, Search, news, fundamentals, corporate actions, and bounded upstream market-data subscriptions through 38 tools, three fixed resources, one resource template, and five prompts.

The server uses MCP over stdio. Its finite subscription tools connect to upstream Tiingo WebSockets; MCP Streamable HTTP and WebSocket transports are not part of this server.

## Installation

### Release installer

On macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/major7apps/tiingo-mcp/releases/download/v2.0.2/tiingo-mcp-installer.sh | sh
```

On Windows PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/major7apps/tiingo-mcp/releases/download/v2.0.2/tiingo-mcp-installer.ps1 | iex"
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
claude mcp add tiingo --env TIINGO_API_KEY=your-api-key-here -- tiingo-mcp
```

You can also launch the server directly:

```bash
TIINGO_API_KEY=your-api-key-here tiingo-mcp
```

`tiingo-mcp` starts the stdio MCP server by default, writes protocol messages only to stdout, and sends diagnostics to stderr. It exits cleanly when stdin closes.

## Tools

### Stocks (EOD and lifecycle metadata)

| Tool | Description |
|---|---|
| `get_stock_metadata` | Per-ticker name, exchange, description, and EOD date range |
| `get_stock_prices` | Historical raw and adjusted EOD OHLCV, dividends, and splits |
| `get_bulk_eod_prices` | Typed bulk CSV refresh with raw/adjusted fields and history-refresh tickers |
| `get_ticker_metadata` | Selected vendor-supplied lifecycle/security-master columns |

### IEX REST

| Tool | Description |
|---|---|
| `get_realtime_price` | Current per-ticker IEX snapshot |
| `get_intraday_prices` | Per-ticker IEX intraday history and optional columns |
| `get_iex_market_snapshot` | All-market IEX snapshot; use deliberately because the response can be large |

### Consolidated equity REST beta (4am–8pm ET)

| Tool | Description |
|---|---|
| `get_equity_realtime_snapshot` | Consolidated ticker or all-market snapshot |
| `get_equity_intraday_prices` | Consolidated intraday history with resampling, after-hours, fill, and column filters |

### BOATS REST beta/add-on (8pm–3:59am ET)

| Tool | Description |
|---|---|
| `get_boats_snapshot` | BOATS ticker or all-market snapshot |
| `get_boats_prices` | BOATS intraday history with resampling, after-hours, and column filters |

### Funds

| Tool | Description |
|---|---|
| `get_fund_metadata` | Mutual-fund or ETF fee metadata |
| `get_fund_fee_metrics` | Current and historical mutual-fund or ETF fee metrics |

### Search early beta

| Tool | Description |
|---|---|
| `search_tiingo_assets` | Bounded asset search by ticker or name |

### Crypto Yield

| Tool | Description |
|---|---|
| `get_crypto_yield_platforms` | Lending-platform list with optional platform filters |
| `get_crypto_yield_pools` | Lending-pool metadata with pool/platform filters |
| `get_crypto_yield_ticks` | Latest lending-pool metric ticks |
| `get_crypto_yield_metrics` | Historical OHLC metrics for one lending pool |

### Forex beta

| Tool | Description |
|---|---|
| `get_forex_quote` | Current top-of-book rate for one pair |
| `get_forex_quotes` | Batch top-of-book rates for 1–100 pairs |
| `get_forex_prices` | Historical forex prices |

### Crypto

| Tool | Description |
|---|---|
| `get_crypto_quote` | Current prices for one or more crypto tickers |
| `get_crypto_prices` | Historical crypto prices |
| `get_crypto_metadata` | Ticker metadata and supported exchanges |

### News

| Tool | Description |
|---|---|
| `get_news` | Search financial articles by ticker, tag, source, or date |

### Fundamentals

| Tool | Description |
|---|---|
| `get_fundamentals_definitions` | Tiingo fundamental metric definitions |
| `get_financial_statements` | Income statements, balance sheets, and cash-flow statements |
| `get_daily_fundamentals` | Daily market and valuation metrics with optional columns |
| `get_company_meta` | Company sector, industry, and location metadata with optional columns |

### Corporate actions

| Tool | Description |
|---|---|
| `get_distributions_by_ex_date` | Cross-ticker distributions for an optional exact ex-date, including announced events |
| `get_dividends` | Per-ticker dividend and distribution history |
| `get_dividend_yield` | Per-ticker dividend-yield history |
| `get_splits` | Per-ticker split history |
| `get_splits_by_ex_date` | Cross-ticker splits for an optional exact ex-date, including announced/cancelled events |

### Finite upstream market-data lifecycle

| Tool | Description |
|---|---|
| `start_market_data_subscription` | Start one bounded IEX or consolidated-equity upstream subscription |
| `poll_market_data_subscription` | Poll retained events by local arrival sequence with finite limits/wait |
| `update_market_data_subscription` | Add or remove explicit symbols on an active subscription |
| `stop_market_data_subscription` | Idempotently unsubscribe, close, cancel, and join the worker |

## EOD cache workflow

Seed each ticker's history from `/tiingo/daily/{ticker}/prices`. Refresh daily with bulk CSV `/tiingo/daily/prices`; the server returns typed JSON that keeps raw and adjusted OHLCV plus `splitFactor` and `divCash` separately named. If a refresh row has `splitFactor != 1` or `divCash > 0`, reseed that ticker's cached history so its adjusted series reflects the corporate action.

## Access and quota

Access is determined by the capabilities attached to the caller's Tiingo key; this project does not promise access from a named plan. HTTP 401 means Tiingo rejected the credential. HTTP 403 means the credential is valid but the account is not entitled to the requested capability.

- IEX upstream subscriptions default to derived-reference threshold 6. Levels 0 and 5 are accepted only when the caller explicitly confirms a direct IEX market-data agreement.
- Consolidated equity is beta, operates 4am–8pm ET, and supports threshold 6 reference ticks or threshold 4 liquidity/top-of-book derived data.
- BOATS REST is a separate beta/add-on for 8pm–3:59am ET. It is not combined with consolidated equity into a unified 24x5 endpoint.
- Fund-fee data is restricted to enterprise/institutional access. Fundamentals and corporate actions are entitlement dependent.
- Search is early beta. Crypto Yield is plan/entitlement dependent. `/tiingo/daily/meta` is vendor-supplied and availability dependent; a 404 does not imply another route.

Every live REST call consumes quota and bandwidth, and every live upstream subscription consumes bandwidth. Filter tickers and dates. Bulk and all-market operations are deterministic-test only by default, not routine smoke tests.

## Resources

The server exposes static reference data without making Tiingo API calls.

| Resource | Description |
|----------|-------------|
| `tiingo://capabilities` | Server capabilities and source-dated entitlement guidance |
| `tiingo://fundamentals/definitions` | Curated reference for common fundamental metrics |
| `tiingo://guide/date-formats` | Date formats, resample frequencies, sort options, and parameters |
| `tiingo://guide/{asset_class}` | Guide template for stocks, market data, forex, crypto, Crypto Yield, funds, Search, news, fundamentals, and corporate actions |

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
cargo llvm-cov --all-targets --all-features --locked --fail-under-lines 93 --summary-only
cargo build --release --locked
cargo deny check all
```

The normal test suite uses local mock servers and does not need a Tiingo key. Ignored live-smoke tests require `TIINGO_API_KEY`, explicit authorization, and consume API quota or bandwidth. See [ARCHITECTURE.md](ARCHITECTURE.md), [API_SURFACE.md](API_SURFACE.md), and [QUALITY.md](QUALITY.md) for maintained engineering and evidence contracts.

## License

[MIT](LICENSE)

## Links

- [Tiingo API documentation](https://www.tiingo.com/documentation/general/overview)
- [Model Context Protocol](https://modelcontextprotocol.io)
- [RMCP](https://github.com/modelcontextprotocol/rust-sdk)
- [Source repository](https://github.com/major7apps/tiingo-mcp)
