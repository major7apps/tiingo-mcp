# API surface

This is the maintained tool-to-upstream contract for the current 38-tool server. The original 17 tool names, required inputs, omission behavior, and results remain compatible with `tests/contract/baseline/v1-mcp.json`. The approved optional input-schema extensions below are explicit; the frozen baseline is not rewritten. The server still exposes 34 REST tools and four finite upstream WebSocket lifecycle tools.

Status means:

- **documented** — present in Tiingo's public endpoint documentation;
- **beta** — documented by Tiingo as beta or early beta;
- **vendor-supplied** — supplied for this integration but not in the public endpoint catalog; and
- **lifecycle** — a finite local operation over an implemented upstream WebSocket service.

Test/access classes are `D` (deterministic-testable), `L` (bounded read-only live-testable), `E` (entitlement or availability dependent), and `Q` (consumes live quota or bandwidth). These classes describe what can be tested safely; `L` does not claim that every row has its own checked-in ignored test. Every live call is also `Q`. Bulk and all-market operations have no routine live smoke.

## Approved REST controls

Omitted optional controls are not sent upstream, preserving existing defaults and route selection. Explicit `false` values are forwarded. MCP argument names use snake case; upstream query names use the camel case shown below.

| Operation | Optional MCP controls | Tiingo query names and bounds |
|---|---|---|
| Stock EOD history | `columns`, `sort` | `columns` contains 1–32 identifiers; `sort` selects an ascending field or a field prefixed by `-` |
| Financial statements | `as_reported`, `sort` | `asReported`; only `date` or `-date` sorting |
| Daily fundamentals | `sort` | `sort`, including `date` or `-date` |
| Asset search | `exact_ticker_match`, `include_delisted`, `limit` | `exactTickerMatch`, `includeDelisted`, `limit` between 1 and 100 |
| IEX history | `after_hours`, `force_fill` | `afterHours`, `forceFill` |
| BOATS history | `force_fill` | `forceFill`; existing `after_hours` does not widen the overnight session |
| IEX, consolidated and BOATS snapshots | `tickers` | 1–100 explicit tickers serialized as `tickers=aapl,spy`; existing `ticker` and new `tickers` are mutually exclusive where both are offered |
| Crypto current/history prices | `exchanges`; current quotes also accept `resample_freq` | `exchanges` contains 1–100 exchange identifiers; `resampleFreq` selects the interval |
| Intraday history and crypto quotes | `resample_freq` | Canonical positive integer minutes/hours, e.g. `45min` or `4hour`; crypto and Crypto Yield also accept day multiples, e.g. `2day` |

IEX history accepts minute/hour units only. Consolidated equity, BOATS and forex also retain the previously accepted `1day`, but reject other day multiples because their current official tables document minute/hour units. Crypto and Crypto Yield accept day multiples. The positive integer must fit a 32-bit unsigned value; zero, negative, fractional, padded, uppercase-unit and leading-zero intervals are rejected locally. EOD history retains `daily`, `weekly`, `monthly`, and `annually`.

The exact approved changes to legacy descriptors are:

- Existing optional `columns` on `get_intraday_prices`, `get_daily_fundamentals`, `get_company_meta`, and `get_dividend_yield`.
- Optional `columns` and `sort` on `get_stock_prices`; `after_hours` and `force_fill` on `get_intraday_prices`; `as_reported` and `sort` on `get_financial_statements`; and `sort` on `get_daily_fundamentals`.
- Optional `exchanges` and `resample_freq` on `get_crypto_quote`; optional `exchanges` on `get_crypto_prices`.
- Broader valid `resample_freq` strings on `get_intraday_prices`, `get_forex_prices`, and `get_crypto_prices`, retaining every previously accepted value.

MCP discovery describes these controls in both tool text and field descriptions, including bounds, omitted defaults, mutually exclusive snapshot filters, and the expanded interval syntax. Compatibility tests permit only these named metadata corrections while preserving the frozen oracle and unrelated descriptor fields.

With `as_reported=true`, Tiingo documents statements as released, with SEC filing publication dates. When omitted or false, Tiingo returns the latest revisions with fiscal-period dates. This is a request mode; it does not establish a complete point-in-time dataset or independent accounting accuracy.

## Implemented tools

| Tool | Tiingo route or upstream service | Status | Access and entitlement | Test/access class |
|---|---|---|---|---|
| `get_stock_metadata` | `GET /tiingo/daily/{ticker}` | documented | Key capability dependent | D, L, Q |
| `get_stock_prices` | `GET /tiingo/daily/{ticker}/prices` | documented | Key capability dependent | D, L, Q |
| `get_bulk_eod_prices` | `GET /tiingo/daily/prices?format=csv` | documented | Bulk response; typed JSON output; no routine live smoke | D, Q |
| `get_ticker_metadata` | `GET /tiingo/daily/meta?columns=...` | vendor-supplied | Availability dependent; 1–32 allowlisted columns; no bulk live smoke | D, E, Q |
| `get_realtime_price` | `GET /iex/{ticker}` | documented | Full TOPS fields require IEX entitlement | D, L, E, Q |
| `get_intraday_prices` | `GET /iex/{ticker}/prices` | documented | Key capability dependent | D, L, Q |
| `get_iex_market_snapshot` | `GET /iex[?tickers=...]` | documented | Explicit 1–100 ticker filter or unchanged all-market default; no unfiltered live smoke | D, L, E, Q |
| `get_equity_realtime_snapshot` | `GET /tiingo/equity/intraday[/{ticker}][?tickers=...]` | beta | Single ticker or explicit 1–100 ticker filter; no unfiltered live smoke | D, L, Q |
| `get_equity_intraday_prices` | `GET /tiingo/equity/intraday/{ticker}/prices` | beta | Consolidated 4am–8pm ET product | D, L, Q |
| `get_boats_snapshot` | `GET /boats[/{ticker}][?tickers=...]` | beta | Separate BOATS add-on; single ticker or explicit 1–100 ticker filter | D, L, E, Q |
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
| `poll_market_data_subscription` | Existing local subscription queue | lifecycle | Bounded cursor poll with sanitized terminal classification; no new upstream subscription | D, L, Q |
| `update_market_data_subscription` | Upstream update using acknowledged subscription ID | lifecycle | Add/remove explicit symbols; partial failures report `appliedSymbols`; threshold changes require stop/start | D, L, Q |
| `stop_market_data_subscription` | Best-effort upstream unsubscribe and local cleanup | lifecycle | Idempotent; never exposes upstream subscription ID | D, L, Q |

## Checked-in ignored live smokes

This inventory records the live harness that is actually checked in. It is intentionally narrower than the `L` capability classification above. Every row requires `TIINGO_API_KEY`, is ignored by default, and consumes quota or bandwidth. `tests/support/live_cases.rs` declares the cases and represented tools used by execution and documentation checks. An inventory entry is a runnable case, not evidence that it has been run or returned data. An early entitlement return can skip later represented tools; record those as skipped, not denied.

| Ignored test | Tools exercised | Bound |
|---|---|---|
| `live_boats_single_ticker` | `get_boats_snapshot`, `get_boats_prices` | One ticker; recent seven-day OHLCV history; entitlement-classifying |
| `live_consolidated_equity_single_ticker` | `get_equity_realtime_snapshot`, `get_equity_intraday_prices` | One ticker; recent seven-day OHLCV history; documented session |
| `live_consolidated_level_six_single_ticker_websocket` | `start_market_data_subscription`, `poll_market_data_subscription`, `stop_market_data_subscription` | Three level-6 one-ticker lifecycles; finite poll/cleanup |
| `live_crypto_yield_metrics_single_pool` | `get_crypto_yield_metrics` | One pool/date range; three samples or early 403 |
| `live_distributions_by_ex_date_tiny_filter` | `get_distributions_by_ex_date` | One exact ex-date; three samples or early 403 |
| `live_forex_quotes_single_pair` | `get_forex_quotes` | One pair; three samples or early 403 |
| `live_fund_fees_single_ticker` | `get_fund_metadata`, `get_fund_fee_metrics` | One ticker; three samples per operation or early 403 |
| `live_iex_level_six_single_ticker_websocket` | `start_market_data_subscription`, `poll_market_data_subscription`, `stop_market_data_subscription` | Three level-6 one-ticker lifecycles; finite poll/cleanup |
| `live_mcp_bounded_rest_end_to_end` | `get_stock_metadata`, `get_stock_prices`, `get_realtime_price`, `get_intraday_prices`, `get_iex_market_snapshot`, `get_equity_realtime_snapshot`, `get_equity_intraday_prices`, `get_boats_snapshot`, `get_boats_prices`, `get_fund_metadata`, `get_fund_fee_metrics`, `search_tiingo_assets`, `get_crypto_yield_platforms`, `get_crypto_yield_pools`, `get_crypto_yield_ticks`, `get_crypto_yield_metrics`, `get_forex_quote`, `get_forex_quotes`, `get_forex_prices`, `get_crypto_quote`, `get_crypto_prices`, `get_crypto_metadata`, `get_news`, `get_fundamentals_definitions`, `get_financial_statements`, `get_daily_fundamentals`, `get_company_meta`, `get_distributions_by_ex_date`, `get_dividends`, `get_dividend_yield`, `get_splits`, `get_splits_by_ex_date` | Actual stdio child; 32 REST calls with filters where supported; 10-second outer call timeout; up to 96 HTTP attempts with retries |
| `live_mcp_iex_subscription_update_end_to_end` | `start_market_data_subscription`, `update_market_data_subscription`, `poll_market_data_subscription`, `stop_market_data_subscription` | Actual stdio child; one level-6 lifecycle, AAPL to MSFT; 10 seconds per call; poll at most one event/1000 ms; stop and child cleanup |
| `live_mcp_consolidated_subscription_update_end_to_end` | `start_market_data_subscription`, `update_market_data_subscription`, `poll_market_data_subscription`, `stop_market_data_subscription` | Actual stdio child; one level-6 lifecycle, AAPL to MSFT; 10 seconds per call; poll at most one event/1000 ms; stop and child cleanup |
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
| Small Exchange REST | deferred | Public legacy tables describe `/smallx/meta`, `/smallx/tops`, `/smallx/{ticker}/prices`, and `/smallx/{ticker}/eod`; current feed availability and licensing need vendor confirmation. |
| Small Exchange WebSocket | deferred | No published upstream WebSocket URL/frame contract was validated. |
| Unified 24x5 equities | deliberately excluded | No single documented route exists; consolidated and BOATS remain separate products and sessions. |
| BOATS WebSocket | deferred | Audited level-3 add-on, outside the approved upstream streaming slice. |
| Crypto WebSocket | deferred | Published thresholds 2/5 and per-exchange frames; bounded ticker subscription and acknowledgement semantics need confirmation before adding a finite lifecycle. |
| Forex WebSocket | deferred | Official prose/examples use threshold 5 while the request table uses 7; vendor clarification is required. |
| Legacy test WebSocket | deliberately excluded | Old authentication syntax and no product contract. |
| Fund fee history date/resample controls | deferred | The public table reuses intraday OHLC wording; fee-event date and interval semantics need vendor confirmation. |
| Crypto raw exchange/currency conversion controls | deferred | `includeRawExchangeData`, `baseCurrency`, and `convertCurrency` are not unambiguously documented as active prices-route request controls. |

## Official sources

`sources_as_of: 2026-10-04`. The public request tables and upstream WebSocket layouts were reviewed against Tiingo’s documentation module. Entitlements and beta availability can change; documentation does not prove the configured account’s current access.

- [Tiingo API overview](https://www.tiingo.com/documentation/general/overview)
- [End-of-Day](https://www.tiingo.com/documentation/end-of-day), the exact [bulk-ingest and corporate-action reseed workflow](https://www.tiingo.com/kb/article/the-fastest-method-to-ingest-tiingo-end-of-day-stock-api-data/), [IEX REST](https://www.tiingo.com/documentation/iex), [consolidated equity REST](https://www.tiingo.com/documentation/equity-realtime-stock-data), and [BOATS REST](https://www.tiingo.com/documentation/boats)
- [Forex](https://www.tiingo.com/documentation/forex), [crypto](https://www.tiingo.com/documentation/crypto), [Crypto Yield](https://www.tiingo.com/documentation/crypto-yield), [news](https://www.tiingo.com/documentation/news), and [Search](https://www.tiingo.com/documentation/utilities/search)
- [Fundamentals](https://www.tiingo.com/documentation/fundamentals), [fund fees](https://www.tiingo.com/documentation/mutual-fund-and-etf-fees), [dividends](https://www.tiingo.com/documentation/corporate-actions/dividends), and [splits](https://www.tiingo.com/documentation/corporate-actions/splits)
- [IEX WebSocket](https://www.tiingo.com/documentation/websockets/iex), [consolidated equity WebSocket](https://www.tiingo.com/documentation/websockets/equity-realtime-stock-data), and the deferred [BOATS](https://www.tiingo.com/documentation/websockets/boats), [crypto](https://www.tiingo.com/documentation/websockets/crypto), [forex](https://www.tiingo.com/documentation/websockets/forex), and [Small Exchange REST](https://www.tiingo.com/documentation/small-exchange) surfaces
- [Official documentation module inspected on 2026-10-04](https://apimedia.tiingo.com/dist/src_app_api_documentation_documentation_module_ts-es2015.50ea0511d8c3fba16fdf.js)

See [ARCHITECTURE.md](ARCHITECTURE.md) for lifecycle bounds and [QUALITY.md](QUALITY.md) for evidence requirements.
