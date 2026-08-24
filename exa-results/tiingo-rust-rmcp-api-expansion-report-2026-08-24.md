# tiingo-mcp: Rust/RMCP migration and Tiingo API expansion report

Date: 2026-08-24

> **Historical discovery report.** This report predates the approved cutover design and records options considered during research. The implemented 2.0 architecture is the single Cargo package at the repository root, uses stdio only, removes PyPI/`uvx` compatibility, and includes bounded retries and an 8 MiB response limit. The approved design in `docs/superpowers/specs/2026-08-24-tiingo-rust-rmcp-parity-cutover-design.md` is authoritative where this report differs.

## Executive decision

A complete Rust rewrite is feasible. RMCP is no longer the experimental part of the decision: it is the official Rust MCP SDK, the MCP project currently classifies Rust as Tier 1, and `rmcp` 3.1.4 supports tools, resources, prompts, stdio, Streamable HTTP, subscriptions, tasks, structured results, and current protocol negotiation. The current server's 17 tools, four resources, five prompts, lifecycle, and stdio transport all have direct RMCP equivalents. ([MCP SDK tiers](https://modelcontextprotocol.io/docs/sdk), [RMCP repository](https://github.com/modelcontextprotocol/rust-sdk), [rmcp 3.1.4](https://crates.io/crates/rmcp))

The rewrite is not justified by request latency alone. This server is mostly a network-bound adapter around Tiingo, whose rate limits and response times dominate local Python overhead. Rust's project-specific benefits would instead be:

- one native executable with no Python 3.12/`uvx` runtime dependency;
- stronger request/schema/error modeling as the endpoint count roughly doubles;
- a natural Tokio-based home for five high-volume Tiingo WebSocket families;
- lower startup and memory overhead in principle, subject to measurement rather than assumption;
- easier embedding in an existing Rust service or gateway.

The costs are a new cross-platform release pipeline, more MCP boilerplate than FastMCP, a complete compatibility rewrite, and loss of today's extremely convenient PyPI/`uvx` install path unless a binary-wheel shim is retained. FastMCP 3.2.3 already supports stdio, Streamable HTTP, resources, prompts, typed structured results, and production HTTP deployment, so neither remote transport nor protocol completeness requires Rust. ([FastMCP server](https://gofastmcp.com/servers/server), [FastMCP HTTP deployment](https://gofastmcp.com/deployment/http))

**Recommendation:** if the product goal is only broader REST coverage, keep Python and expand it. If the strategic goal includes native distribution, an always-on remote service, or Tiingo WebSocket ingestion, adopt Rust/RMCP now—but use a parity-first migration and expand the API only after the existing MCP contract is proven equivalent. Do not combine an unverified rewrite with dozens of new endpoints in one big-bang release.

## Research and verification basis

- Complete sweep of all 23 tracked project files, including implementation, prompts, resources, tests, packaging, Smithery configuration, CI, and release metadata.
- Local authority: `main` at `bf45242`, matching `origin/main`, initially clean.
- Runtime resolved from the lock/environment: FastMCP 3.2.3, HTTPX 0.28.1, Pydantic 2.13.0.
- Verification: Ruff lint passed; Ruff formatting check passed; `137 passed, 4 skipped` in 5.05 seconds. The four skips were the paid/entitlement corporate-action live cases. The worktree remained clean after verification.
- Exa research: 27 searches, 164 results reviewed across four workstreams, deduplicated to 101 unique URLs. Final claims weight official Tiingo, MCP, RMCP, FastMCP, HTTPX, crates.io, and repository sources; third-party generated API indexes were used only for discovery, not as authoritative evidence.

## Current project map

The codebase is small and coherent:

| Area | Current implementation | Assessment |
|---|---|---|
| Upstream client | One `httpx.AsyncClient`, 17 endpoint methods, token header, 30-second timeout | Simple and readable; responses and most inputs are untyped |
| MCP server | 17 FastMCP tools, lazy singleton client, shared `_safe_call`, stdio entry point | Easy to follow; returns JSON strings rather than structured MCP results |
| Resources | Three fixed URIs plus one six-value URI template | Useful, but plan/rate-limit facts are hardcoded and already stale |
| Prompts | Five static workflow prompts | Well tested, but three ask the model to infer unsupported conclusions |
| Tests | Mock client tests, in-memory MCP tests, resource/prompt tests, live integration tests | Good baseline; CI runs only client/server unit subsets |
| Distribution | PyPI wheel/sdist, `uvx tiingo-mcp`, Smithery stdio config | Excellent local install UX; no actual HTTP launch/config path |
| CI/release | Python 3.12/3.13 lint, format, selected tests; tag-triggered trusted PyPI publish | Clean and small; omits resource/prompt tests and all live contract smoke tests |

The implementation is about 1,323 source lines plus about 1,354 test lines. That size makes a rewrite practical, but it does not make regression risk disappear: the externally visible MCP schema, tool names, error behavior, and install contract matter more than line count.

## Sweep findings that should be fixed regardless of language

### P0: correctness and protocol behavior

1. **The entitlement and rate-limit documentation is stale.** The README and capabilities resource say Power is 5,000 requests/hour and 50,000/day and broadly describe fundamentals as free. Tiingo's current pricing page says Power is 10,000/hour and 100,000/day, while its fundamentals documentation says full access is an add-on and only a limited DOW 30 evaluation set is free. BOATS is a separate paid entitlement, and fund fees are enterprise/institutional. Static plan assertions should be removed or stamped with a source date. ([Tiingo pricing](https://www.tiingo.com/account/billing/pricing), [fundamentals](https://www.tiingo.com/documentation/fundamentals), [BOATS](https://www.tiingo.com/documentation/boats), [fund fees](https://www.tiingo.com/documentation/mutual-fund-and-etf-fees))

2. **The crypto quote tool uses a deprecated upstream endpoint.** `get_crypto_quote` calls `/tiingo/crypto/top`, while Tiingo warns that the broad consolidated top-of-book endpoint is deprecated because exchange feeds were not reliable enough for consistent best-bid/offer construction. Preserve the MCP tool name for compatibility, but move its default behavior to the current `/prices` endpoint or clearly expose a deprecated mode. ([Tiingo crypto documentation](https://www.tiingo.com/documentation/crypto))

3. **Share-class symbology is wrong in the resource guide.** It says `BRK.B`; Tiingo's current general documentation says its API uses dashes, such as `BRK-A`, rather than periods. ([Tiingo overview](https://www.tiingo.com/documentation/general/overview))

4. **“Retries=2” is narrower than the documentation claims.** HTTPX transport retries cover `ConnectError` and `ConnectTimeout`; they do not retry 429, read/write failures, or 503 responses. The server currently raises immediately for those status codes. Add a bounded, GET-only retry policy with `Retry-After`, jittered backoff, and an explicit attempt cap, or describe the behavior accurately. ([HTTPX transports](https://www.python-httpx.org/advanced/transports/))

5. **Upstream failures look like successful MCP calls.** `_safe_call` catches errors and returns a JSON string such as `{"error": ...}`. MCP defines recoverable tool failures as `isError: true`; successful structured data belongs in `structuredContent`, optionally mirrored as text for compatibility. FastMCP already supports both patterns. ([MCP tool results](https://modelcontextprotocol.io/specification/2025-06-18/server/tools), [FastMCP tools](https://gofastmcp.com/servers/tools))

6. **The public description overclaims coverage.** `server.py` says it exposes the “full Tiingo financial data API,” but multiple documented families and major request parameters are absent.

### P1: model and operational quality

7. **Large responses have no token or payload guardrail.** All results are pretty-printed into text. Full-market crypto, IEX, Equity Realtime, BOATS, bulk news, and long historical requests can be enormous. Responses need row/event caps, pagination metadata, selectable columns, and resource links or bounded downloads for bulk data.

8. **Input validation is minimal.** Dates, resampling values, sort values, tickers, limits, columns, and booleans are forwarded as raw strings. Rust request types plus `schemars`, or Pydantic models in the current server, can make invalid combinations fail before consuming a Tiingo request.

9. **Pydantic is declared but not used directly.** Every upstream response is `Any`/`dict`. A full strict response-model rewrite would be brittle because Tiingo explicitly adds fields over time; use typed request models and stable response envelopes while preserving unknown JSON fields.

10. **The prompt contract can induce unsupported claims.** `analyze-stock` asks for sector without calling company metadata; `earnings-report-analysis` asks “beat or miss” without estimates/consensus data; `forex-pair-analysis` asks for likely causes using only price data. Either add the required data source/tool or narrow the requested conclusions.

11. **CI does not run the full offline suite.** It explicitly runs only `test_client.py` and `test_server.py`, excluding resource and prompt tests. Live tests are necessarily credentialed, but one low-quota scheduled smoke per entitled family would catch documentation/API drift.

12. **Distribution metadata can accept an unbounded FastMCP major upgrade.** `fastmcp>=2.0.0` allowed the environment to resolve 3.2.3 and could later resolve another breaking major. The lock protects repository CI, not all downstream PyPI installs. Use a compatible upper bound.

13. **HTTP support is documented but not exposed by the shipped entry point.** `main()` calls `mcp.run()` with default stdio. FastMCP can run HTTP, but the project has no CLI flag or deployment configuration selecting it. The changelog's “stdio and HTTP transport support” therefore describes framework capability, not shipped behavior.

## Tiingo API coverage matrix

The confirmed public documentation is substantially larger than the current 17-tool surface.

| Family | Current coverage | Missing or incomplete surface | Status / recommendation |
|---|---|---|---|
| Common REST behavior | JSON only; limited date/resample params | `columns`, response-format policy, consistent pagination/limits, stable permaTicker identity | Add shared request types; keep JSON as MCP default rather than blindly exposing CSV |
| EOD equity/fund prices | Metadata and price history | Column selection, documented format options, better mutual-fund/NAV guidance | Enhance existing tools |
| IEX | One ticker snapshot and historical bars | All/multi-ticker snapshot, `columns`, intraday `afterHours`, `forceFill`, explicit derived-price vs entitled TOPS behavior | High priority; current IEX licensing rules must be reflected ([docs](https://www.tiingo.com/documentation/iex)) |
| Equity Realtime | None | All/specific consolidated snapshot, historical intraday bars, WebSocket reference-price and liquidity streams | New beta product announced 2026-07-07; high-value REST phase ([REST](https://www.tiingo.com/documentation/equity-realtime-stock-data), [WebSocket](https://www.tiingo.com/documentation/websockets/equity-realtime-stock-data)) |
| BOATS overnight | None | All/specific overnight snapshot, historical overnight bars, full WebSocket firehose | New beta product announced 2026-07-21; separate entitlement; high-value opt-in ([REST](https://www.tiingo.com/documentation/boats), [WebSocket](https://www.tiingo.com/documentation/websockets/boats)) |
| Forex | Single-pair quote and history | Batch quote endpoint, columns/filter completeness, WebSocket feed | Enhance REST first, stream later ([REST](https://www.tiingo.com/documentation/forex), [WebSocket](https://www.tiingo.com/documentation/websockets/forex)) |
| Crypto core | Metadata, deprecated top-of-book, historical/current prices | Exchange filters, raw exchange data, safe current-price replacement, WebSocket feed | Replace deprecated default and add filters before new products ([REST](https://www.tiingo.com/documentation/crypto), [WebSocket](https://www.tiingo.com/documentation/websockets/crypto)) |
| News | Search endpoint | Bulk file catalog and batch download | Institutional only; expose catalog/link semantics rather than dumping archives into MCP ([docs](https://www.tiingo.com/documentation/news)) |
| Fundamentals | Definitions, statements, daily, meta | Explicit permaTicker support, columns, current entitlement description | Existing route coverage is good; contract/details need work ([docs](https://www.tiingo.com/documentation/fundamentals)) |
| Fund and ETF fees | None | Fund overview/share classes; historical/current fee metrics | Enterprise/institutional only; optional module ([docs](https://www.tiingo.com/documentation/mutual-fund-and-etf-fees)) |
| Corporate actions | Ticker distributions, ticker yield, ticker splits | Batch distributions and batch splits, exact-date filters, status/cancelled-event guidance | Add batch tools; entitlement-aware, not hardcoded by plan name ([dividends](https://www.tiingo.com/documentation/corporate-actions/dividends), [splits](https://www.tiingo.com/documentation/corporate-actions/splits)) |
| Asset search | None | Search by ticker/name, exact match, delisted inclusion, result limit | Early beta but highly useful for agent workflows; add behind a beta annotation ([docs](https://www.tiingo.com/documentation/utilities/search)) |
| Small Exchange | None | Metadata, all/specific tops, intraday history, official EOD history | Five documented REST patterns; confirm present entitlement/product support before committing because the page looks legacy ([docs](https://www.tiingo.com/documentation/small-exchange)) |
| WebSockets | None | IEX, FX, crypto, Equity Realtime, BOATS | Separate architecture phase; never stream an unbounded firehose directly into model context |
| Crypto synthetics/yield | None | Changelog describes institutional synthetic/state-price products; search also surfaced yield routes | Do not promise these in v2 until Tiingo confirms public routes, schemas, and entitlements; the primary public REST documentation was insufficient to validate the whole surface ([changelog](https://www.tiingo.com/documentation/general/changelog)) |

The strongest near-term expansion is not “add every route.” It is: fix existing parameter completeness and correctness, then add asset search, Equity Realtime REST, BOATS REST, batch corporate actions, and fund-fee modules. Streaming and institutional bulk products require different MCP semantics and should be independently gated.

## Rust/RMCP fit

### What maps cleanly

- FastMCP `@mcp.tool` functions map to RMCP `#[tool]` methods with `#[tool_router]`/`#[tool_handler]`.
- The five prompts map to RMCP prompt routing/macros.
- Static resources and the asset-class template map to `ServerHandler` resource/list/read methods.
- The lazy HTTP client becomes an `Arc<TiingoClient>` held by the server handler.
- `httpx` maps to `reqwest`; the lifecycle moves naturally into Rust ownership/drop and explicit graceful shutdown.
- stdio remains the default through RMCP's `transport-io`; Streamable HTTP can be an optional feature/CLI mode.
- RMCP 3.1.4 targets the stable 2026-07-28 protocol, is compatible with earlier protocol releases, requires Rust 1.88, and exposes optional Cargo features so a stdio build need not carry the complete HTTP/auth stack. ([RMCP repository](https://github.com/modelcontextprotocol/rust-sdk), [crate metadata](https://crates.io/crates/rmcp))

### Benefits specific to this roadmap

1. **Endpoint growth becomes more manageable.** Typed parameter structs, enums, `serde`, and `schemars` make each tool's accepted combinations explicit and generate stable JSON Schemas.
2. **Streaming is a first-class runtime concern.** Tokio tasks, cancellation, bounded channels, and backpressure are a good fit for high-frequency WebSockets.
3. **Native deployment is attractive for local MCP.** A prebuilt executable avoids Python installation, virtual environments, and dependency resolution at launch.
4. **Errors can become exhaustive and observable.** A single `TiingoError` enum can distinguish authentication, entitlement, rate-limit, not-found, transient upstream, decode, and transport failures and map them to `isError: true` with retry hints.
5. **RMCP is now supportable.** The official SDK page lists Rust as Tier 1; the crate has stable 3.x releases and current conformance support. This materially changes the risk profile compared with an early RMCP adoption.

### Costs and non-benefits

1. **No demonstrated user-visible latency win.** Tiingo network latency and quotas dominate. Benchmark cold start, RSS, and wrapper overhead; do not sell the rewrite as API-speed work without data.
2. **More release engineering.** A public native tool needs signed/checksummed artifacts for each target, update automation, and a documented installer. `cargo install` shifts compilation cost to users. Preserving `uvx tiingo-mcp` would require a small non-Rust packaging shim or binary wheels.
3. **More framework ceremony.** FastMCP's decorators and in-memory client make this server unusually concise. RMCP offers equivalent capabilities, but resources, prompts, server info, and tests will be more explicit.
4. **Rapid SDK evolution still exists.** RMCP 3.0 shipped with the 2026-07-28 protocol and reached 3.1.4 by 2026-08-20. Tier 1 lowers protocol risk, not upgrade workload.
5. **Rust cannot correct weak product semantics automatically.** Entitlements, unsupported prompt conclusions, large-result policy, and changing upstream fields still require deliberate design.

## Recommended Rust target architecture

Use one workspace and one production binary; avoid code generation because Tiingo does not publish an authoritative OpenAPI document.

```text
crates/tiingo-mcp/
  src/main.rs                 CLI and stdio/HTTP transport selection
  src/config.rs               API key, timeouts, retry and output limits
  src/error.rs                TiingoError and MCP error mapping
  src/client/mod.rs           reqwest client, auth, retry, response envelope
  src/client/eod.rs
  src/client/iex.rs
  src/client/equity.rs
  src/client/boats.rs
  src/client/forex.rs
  src/client/crypto.rs
  src/client/news.rs
  src/client/fundamentals.rs
  src/client/funds.rs
  src/client/corporate_actions.rs
  src/client/utilities.rs
  src/mcp/tools.rs             RMCP tool router and stable compatibility names
  src/mcp/resources.rs         capabilities, guides, dynamic entitlement facts
  src/mcp/prompts.rs           corrected workflow prompts
  src/streaming/mod.rs         bounded WebSocket sessions; later phase only
  tests/contract/              list/call/read/get golden contract tests
  tests/fixtures/              upstream request/response fixtures
```

Design rules:

- Preserve all 17 existing tool names and compatible inputs during the parity release.
- Use typed inputs; return an object envelope such as `{data, meta}` so MCP `structuredContent` remains an object while upstream arrays stay intact.
- Model only stable response fields; preserve additional fields rather than rejecting upstream evolution.
- Keep `TIINGO_API_KEY` compatible and never serialize or log it.
- Apply bounded GET retries only to safe transient failures, respect `Retry-After`, and never retry 401/403/404.
- Add `max_rows`, columns, truncation metadata, and response-size limits before exposing full-market endpoints.
- Keep stdio as the zero-configuration default. Make Streamable HTTP explicit and authenticated.
- Treat entitlement failure as data: return `isError: true` with endpoint, capability, and a non-speculative “account entitlement required” message.

### WebSocket-to-MCP design

Tiingo firehoses and ordinary MCP tools have incompatible lifetimes. A tool call must not run forever or dump every tick into the model. Use two layers:

1. A compatibility tool that samples a bounded number of events or a bounded duration for any feed.
2. A modern session/subscription layer for clients that support current MCP tasks/subscriptions: start, inspect, and stop a stream; expose the latest bounded buffer through `tiingo://streams/{id}`; emit resource updates rather than raw unbounded tool output.

Every stream must require ticker filters where supported, cap buffer length and event rate, handle reconnect/cancellation, and expose dropped-event counts. Stateful streams over horizontally scaled HTTP also require an external session/event store or sticky ownership; defer that until a concrete remote deployment exists.

## Options considered

| Option | Time/risk | Best when | Verdict |
|---|---|---|---|
| Expand current Python server | Lowest; roughly 12–21 engineer-days for correctness, REST, and bounded streaming | API coverage is the only goal and `uvx` UX matters most | Best product-speed option |
| Rust parity first, then expand | Medium; roughly 23–38 engineer-days including release engineering | Native distribution and streaming are strategic | Recommended Rust path |
| Rewrite and expand simultaneously | Highest; contract drift and upstream drift are confounded | No compelling case | Reject |
| Permanent Python MCP front end plus Rust sidecar | Operationally complex for this small repo | A heavy data engine already exists separately | Reject unless another Rust service must own the streams |

The estimates are engineering ranges, not commitments. They assume one experienced engineer, access to the necessary Tiingo entitlements, no new hosted control plane, and preservation of the current five prompts/four resources. The largest uncertainty is streaming and cross-platform distribution, not the REST wrappers.

## Phased execution plan and gates

### Phase 0 — truth and contract freeze (2–3 days)

- Correct stale plan, rate-limit, symbology, HTTP, retry, and crypto deprecation claims.
- Capture `tools/list`, `resources/list`, templates, prompts, and representative calls as golden MCP fixtures.
- Decide the compatibility policy for JSON text versus structured results.
- Confirm Tiingo entitlements for BOATS, fundamentals, fund fees, bulk news, corporate actions, and Small Exchange.

**Gate:** existing server passes the full suite; contract fixtures are reviewed; unconfirmed products are not in the committed scope.

### Phase 1 — Rust parity (6–9 days)

- Build the RMCP stdio server and typed `reqwest` client.
- Port all 17 tools, four resources, five prompts, and error/lifecycle behavior.
- Run old and new servers against the same fixture corpus.

**Gate:** names, input schemas, resource URIs, prompt arguments, successful results, and classified errors match the approved compatibility contract.

### Phase 2 — REST hardening and expansion (7–12 days)

- Add structured outputs, validation, retry/backoff, size caps, columns, and pagination metadata.
- Add asset search, complete existing parameters, Equity Realtime REST, BOATS REST, batch corporate actions, and fund fees.
- Add optional modules only when live entitlement smoke tests can distinguish 403 entitlement failures from implementation defects.

**Gate:** fixture tests for every route/parameter mapping plus at least one authorized live smoke per product family; no future-dated or undocumented fixture assumptions.

### Phase 3 — bounded streaming (5–9 days)

- Add IEX, FX, crypto, Equity Realtime, and BOATS connectors.
- Implement caps, backpressure, reconnect policy, cancellation, and feed-specific threshold validation.
- Expose bounded sample tools first; add task/resource subscriptions only after client compatibility tests.

**Gate:** soak test reconnects and cancellation, verifies no unbounded memory growth, and proves model-facing payload limits.

### Phase 4 — distribution and cutover (3–5 days)

- Build target binaries, checksums/SBOM, release automation, install documentation, and Smithery configuration.
- Decide whether to retain a PyPI shim for `uvx` compatibility or make GitHub/Homebrew the new install contract.
- Run MCP conformance/Inspector checks and dual-server compatibility tests.

**Gate:** supported platforms install from a clean machine; current clients launch the unchanged `tiingo-mcp` command; rollback to Python remains documented for one release.

## Release acceptance criteria

1. All 17 legacy tools, four resources, and five prompts remain discoverable under their existing names/URIs unless a documented major-version change is approved.
2. Every Tiingo route has an exact path/parameter test, not only a mocked response-shape test.
3. 401, 403, 404, 429, timeout, decode, and 5xx cases produce correct MCP error semantics and never leak secrets.
4. CI runs every offline test; live smoke tests are separately scheduled and quota-bounded.
5. Full-market and historical calls have explicit row/byte/event limits and declare truncation.
6. Entitlement/resource documentation is sourced and dated rather than encoded as timeless plan names.
7. Rust artifacts cover the declared operating-system/architecture matrix and have a practical update path.
8. Performance claims are backed by repeatable cold-start, RSS, and wrapper-overhead measurements; upstream network time is reported separately.
9. WebSocket sessions demonstrate bounded memory, backpressure, cancellation, reconnect behavior, and dropped-event observability.
10. The install migration is explicit: preserving `uvx`, replacing it, or supporting it temporarily is a product decision, not an implementation afterthought.

## Bottom line

There is no blocker to a full Rust/RMCP port, and RMCP's current Tier 1 status makes the move defensible. The compelling reason is the future shape of the server—native distribution plus controlled, concurrent market-data streaming—not the current 17 REST wrappers. If those are real goals, this concise repository is at a good migration point. Freeze parity first, port once, then add the new Tiingo families behind truthful entitlement and payload boundaries. If those goals are not real, keep FastMCP: it already has the protocol and transport capabilities, and the same engineering time will buy more customer-visible API coverage.
