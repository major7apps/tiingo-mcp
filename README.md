# tiingo-mcp

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/major7apps/tiingo-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/major7apps/tiingo-mcp/actions/workflows/ci.yml)

tiingo-mcp is a [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server for the [Tiingo](https://www.tiingo.com) financial data API, and it's written in Rust. An MCP client, such as Claude Code or Claude Desktop, starts the server and calls its tools to fetch stock, forex, crypto, fund, news, and fundamentals data from Tiingo.

Version 2.1.0 exposes 38 tools. The server also provides three fixed resources, one resource template, and five prompts. The tools cover these areas:

- End of day (EOD) stock prices, intraday IEX prices, and consolidated and overnight (BOATS) equity prices
- Forex, crypto, Crypto Yield, and mutual fund and ETF fees
- Search, news, fundamentals, and corporate actions such as dividends and splits
- Short, bounded subscriptions to Tiingo's live market data feeds

The server talks to its MCP client over stdio, which means the client starts it as a child process and exchanges messages through standard input and output. The subscription tools open WebSocket connections to Tiingo, but the server itself does not offer an MCP WebSocket or Streamable HTTP transport.

## Installation

Published 2.1.0 installers, MCPB bundles, and the crates.io package expose the 38-tool surface documented below. You can install the server in four ways, and you only need one of them.

### Install script

The install script is the quickest option. It detects your platform, downloads the matching prebuilt binary from the GitHub release, and puts it in Cargo's binary directory, which is `~/.cargo/bin` unless you set `CARGO_HOME`. You don't need Rust installed to use it.

On macOS or Linux, run:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/major7apps/tiingo-mcp/releases/download/v2.1.0/tiingo-mcp-installer.sh | sh
```

On Windows, run this in PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/major7apps/tiingo-mcp/releases/download/v2.1.0/tiingo-mcp-installer.ps1 | iex"
```

Make sure the binary directory is on your `PATH`, so that MCP clients can start the server by the name `tiingo-mcp`.

### Prebuilt binary

You can download a prebuilt binary yourself if you'd rather not pipe a script into your shell. Each [GitHub release](https://github.com/major7apps/tiingo-mcp/releases/tag/v2.1.0) has one archive per platform, and each archive holds the `tiingo-mcp` binary with the README, changelog, and license.

| Platform | Archive |
|---|---|
| macOS on Apple silicon | `tiingo-mcp-aarch64-apple-darwin.tar.xz` |
| macOS on Intel | `tiingo-mcp-x86_64-apple-darwin.tar.xz` |
| Linux on x86_64 | `tiingo-mcp-x86_64-unknown-linux-musl.tar.xz` |
| Linux on ARM64 | `tiingo-mcp-aarch64-unknown-linux-musl.tar.xz` |
| Windows on x86_64 | `tiingo-mcp-x86_64-pc-windows-msvc.zip` |

The Linux binaries are statically linked, so they run on any distribution without extra libraries.

On macOS or Linux, set `TARGET` to the name from the table and run these commands. They download the archive and its checksum, check the archive against the checksum, and copy the binary to `~/.local/bin`.

```bash
TARGET=aarch64-apple-darwin
BASE=https://github.com/major7apps/tiingo-mcp/releases/download/v2.1.0

curl -LO "$BASE/tiingo-mcp-$TARGET.tar.xz"
curl -LO "$BASE/tiingo-mcp-$TARGET.tar.xz.sha256"
shasum -a 256 -c "tiingo-mcp-$TARGET.tar.xz.sha256"

tar -xJf "tiingo-mcp-$TARGET.tar.xz"
mkdir -p ~/.local/bin
install -m 755 "tiingo-mcp-$TARGET/tiingo-mcp" ~/.local/bin/
tiingo-mcp --version
```

You can use `sha256sum -c` in place of `shasum -a 256 -c` on Linux. If `~/.local/bin` isn't on your `PATH`, add it, or copy the binary to another directory that is. If you download the archive with a web browser on macOS, macOS may block the binary the first time it runs. You can clear the block with `xattr -d com.apple.quarantine ~/.local/bin/tiingo-mcp`.

On Windows, download `tiingo-mcp-x86_64-pc-windows-msvc.zip` and extract `tiingo-mcp.exe` to a folder such as `%LOCALAPPDATA%\Programs\tiingo-mcp`. Then add that folder to your user `PATH`, or put the full path to `tiingo-mcp.exe` in your MCP client's configuration. To check the download, compare the output of `Get-FileHash tiingo-mcp-x86_64-pc-windows-msvc.zip` with the value in the matching `.sha256` file.

The release workflow also publishes GitHub build attestations. If you have the GitHub CLI, you can confirm that an archive was built by this repository's release workflow:

```bash
gh attestation verify "tiingo-mcp-$TARGET.tar.xz" -R major7apps/tiingo-mcp
```

### MCPB desktop bundle

An MCPB bundle is a single file that a desktop MCP host, such as Claude Desktop, can install as an extension. Each release has one bundle per platform, named `tiingo-mcp-<target>.mcpb`, where `<target>` is one of the platform names in the table above. Download the bundle for your system, open it with your host's extension installer, and enter your Tiingo API key when the host asks for it. The bundle includes the binary and its MCP configuration, so you don't need to install the binary or set a path separately.

### Cargo

If you have Rust 1.88 or newer, you can build and install the release from crates.io:

```bash
cargo install tiingo-mcp --version 2.1.0 --locked
```

To install the current development version from a local checkout, run:

```bash
cargo install --path . --locked
```

## MCP configuration

You need a Tiingo API key, which you can create at [api.tiingo.com](https://api.tiingo.com). The data you can reach depends on the features enabled for your key, as described in [Access and quota](#access-and-quota).

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

If `tiingo-mcp` isn't on the `PATH` that your client sees, set `command` to the full path of the binary.

In Claude Code, you can add the server with one command:

```bash
claude mcp add tiingo --env TIINGO_API_KEY=your-api-key-here -- tiingo-mcp
```

You can also start the server directly to check that it runs:

```bash
TIINGO_API_KEY=your-api-key-here tiingo-mcp
```

The server writes only MCP protocol messages to stdout, and it writes diagnostic logs to stderr. It exits when stdin closes. You can control the log level with the `RUST_LOG` environment variable, e.g., `RUST_LOG=debug`.

## Tools

Version 2.1.0 keeps the 17 tools from earlier releases compatible. Their names, required inputs, default behavior, and results are unchanged, and the only additions are optional `columns` fields on four of them.

### Stocks (EOD and security metadata)

| Tool | Description |
|---|---|
| `get_stock_metadata` | Name, exchange, description, and EOD date range for one ticker |
| `get_stock_prices` | Historical raw and adjusted EOD prices and volume, with dividends and splits |
| `get_bulk_eod_prices` | The latest EOD prices for all tickers, with a list of tickers whose history needs a refresh |
| `get_ticker_metadata` | Listing and security details supplied by Tiingo's data vendor |

### IEX prices

| Tool | Description |
|---|---|
| `get_realtime_price` | Current IEX top of book price for one ticker, with optional after hours data |
| `get_intraday_prices` | Intraday IEX price history for one ticker, with optional columns |
| `get_iex_market_snapshot` | Current IEX snapshot for every ticker, which can be a large response |

### Consolidated equity prices (beta, 4am to 8pm ET)

| Tool | Description |
|---|---|
| `get_equity_realtime_snapshot` | Current consolidated snapshot for one ticker or for every ticker |
| `get_equity_intraday_prices` | Consolidated intraday history, with options for interval, after hours data, gap filling, and columns |

### Overnight BOATS prices (beta add-on, 8pm to 3:59am ET)

| Tool | Description |
|---|---|
| `get_boats_snapshot` | Current BOATS snapshot for one ticker or for every ticker |
| `get_boats_prices` | BOATS intraday history, with options for interval, after hours data, and columns |

### Funds

| Tool | Description |
|---|---|
| `get_fund_metadata` | Fee metadata for a mutual fund or ETF |
| `get_fund_fee_metrics` | Current and historical fee metrics for a mutual fund or ETF |

### Search (early beta)

| Tool | Description |
|---|---|
| `search_tiingo_assets` | Search for assets by ticker or name, with a limit on result count |

### Crypto Yield

| Tool | Description |
|---|---|
| `get_crypto_yield_platforms` | List of crypto lending platforms, with optional filters |
| `get_crypto_yield_pools` | Metadata for lending pools, filtered by pool or platform |
| `get_crypto_yield_ticks` | Latest metric values for lending pools |
| `get_crypto_yield_metrics` | Historical open, high, low, and close metrics for one lending pool |

### Forex (beta)

| Tool | Description |
|---|---|
| `get_forex_quote` | Current best bid and ask for one currency pair |
| `get_forex_quotes` | Current best bid and ask for 1 to 100 currency pairs |
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
| `get_news` | Search financial news articles by ticker, tag, source, or date |

### Fundamentals

| Tool | Description |
|---|---|
| `get_fundamentals_definitions` | Definitions of Tiingo's fundamental metrics |
| `get_financial_statements` | Income statements, balance sheets, and cash flow statements |
| `get_daily_fundamentals` | Daily market and valuation metrics, with optional columns |
| `get_company_meta` | Company sector, industry, and location, with optional columns |

### Corporate actions

| Tool | Description |
|---|---|
| `get_distributions_by_ex_date` | Distributions across all tickers, optionally for one ex-date, including announced events |
| `get_dividends` | Dividend and distribution history for one ticker |
| `get_dividend_yield` | Dividend yield history for one ticker |
| `get_splits` | Split history for one ticker |
| `get_splits_by_ex_date` | Splits across all tickers, optionally for one ex-date, including announced and cancelled events |

### Live market data subscriptions

The subscription tools let a client read live IEX or consolidated equity data for a limited time. First, the client starts a subscription. Second, it polls for new events, and it can add or remove symbols while the subscription is active. Third, it stops the subscription.

Each subscription holds at most 2,048 events or 8 MiB that the client hasn't read yet. If more data arrives than the subscription can hold, the subscription closes with the status `data_gap`, so the client never loses events without knowing. A subscription also closes after 30 minutes, or after five minutes with no calls from the client.

| Tool | Description |
|---|---|
| `start_market_data_subscription` | Start one IEX or consolidated equity subscription with a size and time limit |
| `poll_market_data_subscription` | Read new events after a sequence number, with a limit on count and wait time; a closed subscription reports the reason in `terminalError` |
| `update_market_data_subscription` | Add or remove symbols on an active subscription; if part of the update fails, the result lists the symbols that are active in `appliedSymbols` |
| `stop_market_data_subscription` | Close the subscription and release its resources, and it's safe to call more than once |

## Keeping a local EOD price cache

You can keep a local copy of EOD price history with two tools. First, load each ticker's full history once with `get_stock_prices`. Second, call `get_bulk_eod_prices` each day to get the latest prices for all tickers. The bulk result keeps raw and adjusted prices in separate fields, with `splitFactor` and `divCash` for each row.

A split or dividend changes a ticker's adjusted history. So if a row has `splitFactor` other than 1 or `divCash` greater than 0, reload that ticker's full history with `get_stock_prices`.

## Access and quota

Tiingo decides which data your key can reach, based on the features enabled for your account, and this project can't promise access based on a plan name. An HTTP 401 error means Tiingo rejected your API key. An HTTP 403 error means the key is valid, but your account doesn't include the data you asked for.

Some data has extra rules:

- IEX subscriptions use data level 6 by default, which gives reference prices. You can request levels 0 or 5 only if you confirm that you have a direct market data agreement with IEX.
- Consolidated equity data is in beta and covers 4am to 8pm ET. It supports reference ticks at level 6 or top of book data at level 4.
- BOATS data is a separate beta add-on that covers 8pm to 3:59am ET. Tiingo doesn't combine it with consolidated equity data into one 24 hour feed.
- Fund fee data needs enterprise or institutional access. Fundamentals, corporate actions, and Crypto Yield depend on your account's features, and Search is in early beta. The ticker metadata from `get_ticker_metadata` comes from a data vendor and may be missing, so a 404 error from it doesn't mean you should try a different endpoint.

Every REST call uses API quota and bandwidth, and every live subscription uses bandwidth. To reduce use, request only the tickers and dates you need. The bulk and all ticker tools return large responses, so call them only when you need all of that data.

## Resources

The server includes reference documents that clients can read without calling the Tiingo API.

| Resource | Description |
|----------|-------------|
| `tiingo://capabilities` | Server capabilities and dated notes on which Tiingo features each tool needs |
| `tiingo://fundamentals/definitions` | Definitions of common fundamental metrics |
| `tiingo://guide/date-formats` | Date formats, resample intervals, sort options, and other parameters |
| `tiingo://guide/{asset_class}` | A guide for one data area, e.g., stocks, forex, crypto, funds, news, or fundamentals |

## Prompts

| Prompt | Arguments | Description |
|--------|-----------|-------------|
| `analyze-stock` | `ticker`, `include_news` | Full analysis of one stock |
| `compare-stocks` | `ticker1`, `ticker2`, `period` | Comparison of two stocks |
| `crypto-market-overview` | `tickers` | Crypto market summary with seven day trends |
| `earnings-report-analysis` | `ticker`, `earnings_date` | Review of an earnings report, the price reaction, and related news |
| `forex-pair-analysis` | `pair`, `period` | Trend and volatility review for a currency pair |

## Results and errors

Each successful tool call returns the result twice, once as JSON text for older clients and once as MCP structured content. When a call fails in a way the client can handle, the server returns an MCP tool error with a short JSON message, and it removes the API key and any sensitive details from that message.

When a subscription closes because of an error, `poll_market_data_subscription` reports one of four reasons in `terminalError`, which are `authentication`, `entitlement`, `transport`, or `protocol`. The server doesn't pass along Tiingo's original error text.

The server retries a request only when the failure is temporary and a retry is safe, and it rejects responses larger than 8 MiB. It never includes the API key or the authorization header in errors that the client can see.

## Development

To build and check the project, clone the repository and run the same checks that CI runs:

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

The normal test suite runs against local mock servers, so it doesn't need a Tiingo key. The live tests are marked as ignored, because they call the real Tiingo API and use quota or bandwidth. Run them only with explicit approval and a `TIINGO_API_KEY`.

For more detail on how the project is built and tested, see [ARCHITECTURE.md](ARCHITECTURE.md), [API_SURFACE.md](API_SURFACE.md), and [QUALITY.md](QUALITY.md).

## License

[MIT](LICENSE)

## Links

- [Tiingo API documentation](https://www.tiingo.com/documentation/general/overview)
- [Model Context Protocol](https://modelcontextprotocol.io)
- [RMCP, the Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
- [Source repository](https://github.com/major7apps/tiingo-mcp)
