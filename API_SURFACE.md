# API surface

This is the maintained tool-to-upstream contract for the current 38-tool server. The original 17 descriptors remain frozen by `tests/contract/baseline/v1-mcp.json`; every other row is additive.

Status means:

- **documented** — present in Tiingo's public endpoint documentation;
- **beta** — documented by Tiingo as beta or early beta;
- **vendor-supplied** — supplied for this integration but not in the public endpoint catalog; and
- **lifecycle** — a finite local operation over an implemented upstream WebSocket service.

Test/access classes are `D` (deterministic-testable), `L` (bounded read-only live-testable), `E` (entitlement or availability dependent), and `Q` (consumes live quota or bandwidth). These classes describe what can be tested safely; `L` does not claim that every row has its own checked-in ignored test. Every live call is also `Q`. Bulk and all-market operations have no routine live smoke.

## Implemented tools

| Tool | Tiingo route or upstream service | Status | Access and entitlement | Test/access class |
|---|---|---|---|---|
| `get_stock_metadata` | `GET /tiingo/daily/{ticker}` | documented | Key capability dependent | D, L, Q |
| `get_stock_prices` | `GET /tiingo/daily/{ticker}/prices` | documented | Key capability dependent | D, L, Q |
| `get_bulk_eod_prices` | `GET /tiingo/daily/prices?format=csv` | documented | Bulk response; typed JSON output; no routine live smoke | D, Q |
| `get_ticker_metadata` | `GET /tiingo/daily/meta?columns=...` | vendor-supplied | Availability dependent; 1–32 allowlisted columns; no bulk live smoke | D, E, Q |
| `get_realtime_price` | `GET /iex/{ticker}` | documented | Full TOPS fields require IEX entitlement | D, L, E, Q |
| `get_intraday_prices` | `GET /iex/{ticker}/prices` | documented | Key capability dependent | D, L, Q |
| `get_iex_market_snapshot` | `GET /iex` | documented | All-market response; IEX fields are entitlement dependent; no routine live smoke | D, E, Q |
| `get_equity_realtime_snapshot` | `GET /tiingo/equity/intraday[/{ticker}]` | beta | Ticker-filtered live check only | D, L, Q |
| `get_equity_intraday_prices` | `GET /tiingo/equity/intraday/{ticker}/prices` | beta | Consolidated 4am–8pm ET product | D, L, Q |
| `get_boats_snapshot` | `GET /boats[/{ticker}]` | beta | Separate BOATS add-on; ticker live check only | D, L, E, Q |
| `get_boats_prices` | `GET /boats/{ticker}/prices` | beta | Separate BOATS add-on, 8pm–3:59am ET | D, L, E, Q |
| `get_fund_metadata` | `GET /tiingo/funds/{ticker}` | documented | Enterprise/institutional fund-fee capability | D, L, E, Q |
| `get_fund_fee_metrics` | `GET /tiingo/funds/{ticker}/metrics` | documented | Enterprise/institutional fund-fee capability | D, L, E, Q |
| `search_tiingo_assets` | `GET /tiingo/utilities/search?query=...` | beta | Early-beta response fields may change | D, L, Q |
| `get_crypto_yield_platforms` | `GET /tiingo/crypto-yield/platforms` | documented | Plan/entitlement dependent | D, E, Q |
| `get_crypto_yield_pools` | `GET /tiingo/crypto-yield/pools` | documented | Plan/entitlement dependent | D, E, Q |
| `get_crypto_yield_ticks` | `GET /tiingo/crypto-yield/ticks` | documented | Plan/entitlement dependent | D, E, Q |
| `get_crypto_yield_metrics` | `GET /tiingo/crypto-yield/{poolCode}/metrics` | documented | Plan/entitlement dependent | D, L, E, Q |
| `get_forex_quote` | `GET /tiingo/fx/{ticker}/top` | beta | Key capability dependent | D, L, Q |
| `get_forex_quotes` | `GET /tiingo/fx/top?tickers=...` | beta | 1–100 explicit pairs | D, L, Q |
| `get_forex_prices` | `GET /tiingo/fx/{ticker}/prices` | beta | Key capability dependent | D, L, Q |
| `get_crypto_quote` | `GET /tiingo/crypto/prices` | documented | Omit tickers only with bulk intent | D, L, Q |
| `get_crypto_prices` | `GET /tiingo/crypto/prices` | documented | Use bounded ticker/date filters for live checks | D, L, Q |
| `get_crypto_metadata` | `GET /tiingo/crypto` | documented | Filtered live check | D, L, Q |
| `get_news` | `GET /tiingo/news` | documented | Dynamic content; use bounded filters | D, L, Q |
| `get_fundamentals_definitions` | `GET /tiingo/fundamentals/definitions` | documented | Fundamentals entitlement dependent | D, L, E, Q |
| `get_financial_statements` | `GET /tiingo/fundamentals/{ticker}/statements` | documented | Fundamentals entitlement dependent | D, L, E, Q |
| `get_daily_fundamentals` | `GET /tiingo/fundamentals/{ticker}/daily` | documented | Fundamentals entitlement dependent | D, L, E, Q |
| `get_company_meta` | `GET /tiingo/fundamentals/meta` | documented | Fundamentals entitlement dependent | D, E, Q |
| `get_distributions_by_ex_date` | `GET /tiingo/corporate-actions/distributions?exDate=...` | beta | Early-release, entitlement-dependent; may include announced future actions | D, L, E, Q |
| `get_dividends` | `GET /tiingo/corporate-actions/{ticker}/distributions` | beta | Early-release and entitlement-dependent | D, L, E, Q |
| `get_dividend_yield` | `GET /tiingo/corporate-actions/{ticker}/distribution-yield` | beta | Early-release and entitlement-dependent | D, L, E, Q |
| `get_splits` | `GET /tiingo/corporate-actions/{ticker}/splits` | beta | Early-release and entitlement-dependent | D, L, E, Q |
| `get_splits_by_ex_date` | `GET /tiingo/corporate-actions/splits?exDate=...` | beta | Early-release, entitlement-dependent; may include announced/cancelled future actions | D, L, E, Q |
| `start_market_data_subscription` | IEX `wss://api.tiingo.com/iex` or consolidated `wss://api.tiingo.com/equity/intraday` | lifecycle | IEX 6 default; 0/5 need direct-agreement confirmation. Consolidated accepts 4/6. | D, L, E, Q |
| `poll_market_data_subscription` | Existing local subscription queue | lifecycle | Bounded cursor poll; no new upstream subscription | D, L, Q |
| `update_market_data_subscription` | Upstream update using acknowledged subscription ID | lifecycle | Add/remove explicit symbols; threshold changes require stop/start | D, L, Q |
| `stop_market_data_subscription` | Best-effort upstream unsubscribe and local cleanup | lifecycle | Idempotent; never exposes upstream subscription ID | D, L, Q |

## Checked-in ignored live smokes

This inventory records the live harness that is actually checked in. It is intentionally narrower than the `L` capability classification above. Every row requires `TIINGO_API_KEY`, is ignored by default, and consumes quota or bandwidth.

| Ignored test | Tools exercised | Bound |
|---|---|---|
| `live_boats_single_ticker` | `get_boats_snapshot`, `get_boats_prices` | One ticker; entitlement-classifying |
| `live_consolidated_equity_single_ticker` | `get_equity_realtime_snapshot`, `get_equity_intraday_prices` | One ticker; documented session |
| `live_consolidated_level_six_single_ticker_websocket` | `start_market_data_subscription`, `poll_market_data_subscription`, `stop_market_data_subscription` | Three level-6 one-ticker lifecycles; finite poll/cleanup |
| `live_crypto_yield_metrics_single_pool` | `get_crypto_yield_metrics` | One pool/date range; three samples or early 403 |
| `live_distributions_by_ex_date_tiny_filter` | `get_distributions_by_ex_date` | One exact ex-date; three samples or early 403 |
| `live_forex_quotes_single_pair` | `get_forex_quotes` | One pair; three samples or early 403 |
| `live_fund_fees_single_ticker` | `get_fund_metadata`, `get_fund_fee_metrics` | One ticker; three samples per operation or early 403 |
| `live_iex_level_six_single_ticker_websocket` | `start_market_data_subscription`, `poll_market_data_subscription`, `stop_market_data_subscription` | Three level-6 one-ticker lifecycles; finite poll/cleanup |
| `live_mcp_eod_data_is_consistent_accurate_and_timely` | `get_stock_prices` | One ticker/date; three MCP samples |
| `live_read_only_tiingo_capabilities` | `get_stock_metadata`, `get_stock_prices`, `get_forex_quote`, `get_crypto_quote`, `get_news`, `get_fundamentals_definitions`, `get_dividends` | Representative bounded baseline calls |
| `live_search_early_beta` | `search_tiingo_assets` | One query; three samples or early 403 |
| `live_splits_by_ex_date_tiny_filter` | `get_splits_by_ex_date` | One exact ex-date; three samples or early 403 |

## Audited but excluded or deferred

| Surface | Classification | Reason |
|---|---|---|
| `GET /tiingo/crypto/top` | deprecated | Tiingo's current crypto prices route is implemented instead. |
| News bulk downloads | deliberately excluded | Institutional, token-bearing download URLs and binary/unbounded payloads do not fit the bounded JSON tool contract. |
| Company descriptions, security-master bulk, raw crypto/DEX/fundamental/yield bulk | deliberately excluded | Current products lack a stable public route contract suitable for this server. |
| Small Exchange REST/WebSocket | deferred | Only a hidden legacy page exists; no published WebSocket URL/frame contract. |
| Unified 24x5 equities | deliberately excluded | No single documented route exists; consolidated and BOATS remain separate products and sessions. |
| BOATS WebSocket | deferred | Audited level-3 add-on, outside the approved upstream streaming slice. |
| Crypto WebSocket | deferred | Audited thresholds 2/5, outside the approved upstream streaming slice. |
| Forex WebSocket | deferred | Official threshold documentation conflicts; implementation waits for vendor clarification. |
| Legacy test WebSocket | deliberately excluded | Old authentication syntax and no product contract. |

## Official sources

Audit date: **2026-08-25**. Entitlements and beta availability can change; current account behavior is authoritative.

- [Tiingo API overview](https://www.tiingo.com/documentation/general/overview)
- [End-of-Day](https://www.tiingo.com/documentation/end-of-day), the exact [bulk-ingest and corporate-action reseed workflow](https://www.tiingo.com/kb/article/the-fastest-method-to-ingest-tiingo-end-of-day-stock-api-data/), [IEX REST](https://www.tiingo.com/documentation/iex), [consolidated equity REST](https://www.tiingo.com/documentation/equity-realtime-stock-data), and [BOATS REST](https://www.tiingo.com/documentation/boats)
- [Forex](https://www.tiingo.com/documentation/forex), [crypto](https://www.tiingo.com/documentation/crypto), [Crypto Yield](https://www.tiingo.com/documentation/crypto-yield), [news](https://www.tiingo.com/documentation/news), and [Search](https://www.tiingo.com/documentation/utilities/search)
- [Fundamentals](https://www.tiingo.com/documentation/fundamentals), [fund fees](https://www.tiingo.com/documentation/mutual-fund-and-etf-fees), [dividends](https://www.tiingo.com/documentation/corporate-actions/dividends), and [splits](https://www.tiingo.com/documentation/corporate-actions/splits)
- [IEX WebSocket](https://www.tiingo.com/documentation/websockets/iex) and [consolidated equity WebSocket](https://www.tiingo.com/documentation/websockets/equity-realtime-stock-data)

See [ARCHITECTURE.md](ARCHITECTURE.md) for lifecycle bounds and [QUALITY.md](QUALITY.md) for evidence requirements.
