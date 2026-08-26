# Changelog

## 2.1.0 (Unreleased)

### API surface

- Added 17 REST tools for bulk EOD refresh, lifecycle metadata, all-market IEX and batch forex quotes, consolidated equity and BOATS, funds, Search, Crypto Yield, and cross-ticker corporate actions while preserving the original 17 tool names, required inputs, omission behavior, and results; four optional `columns` fields are the only approved legacy descriptor additions.
- Added four finite MCP lifecycle tools for bounded upstream IEX and consolidated-equity WebSocket subscriptions; MCP transport remains stdio.
- Documented source-dated entitlement boundaries for IEX agreement levels, consolidated and BOATS sessions, fund fees, fundamentals, corporate actions, Search, Crypto Yield, and vendor-supplied lifecycle metadata.

### Data integrity and safety

- Preserved raw and adjusted EOD OHLCV plus `divCash` and `splitFactor` in typed bulk output, with explicit history-refresh tickers for dividends and non-unit splits.
- Added bounded WebSocket queues, cursor polling, explicit data-gap failures, finite reconnect/liveness/expiry behavior, duplicate and out-of-order flags, partial-update inventory reconciliation, and cancellation-safe worker shutdown.
- Added socket-level 8-MiB frame/message limits, linear exact-size poll admission, sanitized terminal error classifications, structural upstream-ID redaction, and dependency-trace credential isolation.

### Documentation and testing

- Added focused architecture, API-surface, and quality references with mechanical root-link, embedded-resource, public-count, and discovered-tool documentation checks.
- Expanded deterministic REST, RMCP, WebSocket protocol/lifecycle, cancellation, EOF, credential-redaction, consistency, and latency coverage while keeping quota-consuming live checks ignored by default.

## 2.0.2 (2026-08-25)

### Dependencies

- Updated `actions/checkout` and `actions/upload-artifact` to their current major versions.
- Updated `rand` from 0.9 to 0.10 and removed the superseded transitive dependency versions.

### Testing

- Raised overall line coverage from 84.79% to 93.25% and added a 93% stable-CI floor.
- Added end-to-end route, query, text, and structured-response checks for all 17 MCP tools, bringing the MCP tool layer to 100% line coverage.
- Added deterministic MCP consistency and latency checks plus an ignored, one-request live EOD accuracy check.
- Enforced Cargo offline mode for the MSRV and stable test suites after dependency installation.

## 2.0.1 (2026-08-25)

### Maintenance

- Added weekly Dependabot version updates for Cargo and GitHub Actions.
- Made `AGENTS.md` the harness-neutral repository guide while retaining `CLAUDE.md` as a symlink.

## 2.0.0 (2026-08-24)

### Breaking changes

- Replaced the server implementation with a native Rust binary built on RMCP.
- Changed installation and launch to the direct `tiingo-mcp` command; the PyPI and `uvx` contract has been deliberately removed.
- Kept stdio as the MCP transport for 2.0. Streamable HTTP and WebSocket work remains a separate expansion.

### Distribution

- Added cargo-dist shell and PowerShell installers plus native artifacts for Apple Silicon and Intel macOS, ARM64 and x86-64 Linux, and x86-64 Windows.
- Added target-specific MCPB desktop bundles with path-free binary configuration and secure API-key prompting.
- Added Rust 1.88, stable-toolchain, dependency-policy, native-build, version, and MCPB validation gates.

### Protocol and correctness

- Preserved the 17 tools, three fixed resources, one guide template, five prompts, and compatible input schemas.
- Added structured MCP tool results while retaining the JSON text content expected by older clients.
- Corrected recoverable failures to use MCP tool-error semantics and sanitized error content.
- Bounded transient retries to three total attempts, enforced same-origin requests and an 8 MiB response limit, and redacted credentials from all errors.
- Updated crypto quotes to Tiingo's current prices route.
- Corrected stale resource facts and replaced timeless plan guarantees with source-dated capability guidance.
- Corrected stock, earnings, and forex prompts so they request supported metadata and do not infer unsupported conclusions.

## 1.1.0 (2026-04-13)

### Resources (4)

Static reference data exposed as MCP resources — no API calls consumed.

- `tiingo://capabilities` — server capabilities, asset classes, rate limits, plan restrictions
- `tiingo://fundamentals/definitions` — curated reference of 20 fundamental metrics
- `tiingo://guide/date-formats` — date formats, resample frequencies, sort options
- `tiingo://guide/{asset_class}` — per-asset-class usage guide (stocks, forex, crypto, news, fundamentals, corporate-actions)

### Prompts (5)

Reusable analysis workflow templates that guide LLMs through multi-step financial analysis.

- `analyze-stock` — comprehensive single-stock analysis (metadata, prices, fundamentals, news)
- `compare-stocks` — side-by-side comparison of two tickers
- `crypto-market-overview` — crypto market snapshot with 7-day trends
- `earnings-report-analysis` — earnings report analysis with price reaction and news sentiment
- `forex-pair-analysis` — currency pair trend and volatility analysis

## 1.0.0 (2026-04-13)

Initial release.

### Tools (17)

- **EOD Stocks**: `get_stock_metadata`, `get_stock_prices`
- **IEX Real-Time**: `get_realtime_price`, `get_intraday_prices`
- **Forex**: `get_forex_quote`, `get_forex_prices`
- **Crypto**: `get_crypto_quote`, `get_crypto_prices`, `get_crypto_metadata`
- **News**: `get_news`
- **Fundamentals**: `get_fundamentals_definitions`, `get_financial_statements`, `get_daily_fundamentals`, `get_company_meta`
- **Corporate Actions**: `get_dividends`, `get_dividend_yield`, `get_splits`

### Features

- Full async implementation with httpx
- Automatic retries on transient errors
- Structured error handling (401, 403, 404, 429, 5xx)
- PyPI-publishable, installable via `uvx tiingo-mcp`
- stdio and HTTP transport support
