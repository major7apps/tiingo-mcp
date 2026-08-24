# Tiingo MCP Rust/RMCP parity migration and Python cutover

Date: 2026-08-24

Status: Proposed for implementation review

Scope: First delivery of the approved Rust program

## Decision and scope

Replace the current Python/FastMCP implementation with one Rust binary built on the official RMCP SDK. The replacement stays in this repository, keeps `tiingo-mcp` as the executable name, keeps `TIINGO_API_KEY` as the credential contract, and starts an MCP stdio server when invoked with no arguments.

The broader program is deliberately split into three deliveries:

1. Rust/RMCP parity, known correctness fixes, native distribution, and Python cutover.
2. Tiingo REST hardening and API expansion.
3. Bounded Tiingo WebSocket tools and subscriptions.

This specification covers only the first delivery. It creates the typed foundation needed by the later work but does not add new Tiingo product families or long-lived WebSocket sessions.

## Goals

- Reproduce the discoverable contract of all 17 tools, three fixed resources, one resource template, and five prompts.
- Start directly as a local MCP server with `command: "tiingo-mcp"` and no package runner or subcommand.
- Improve safety where the current behavior is known to be wrong: error semantics, transient retries, secret handling, stale resource facts, deprecated crypto routing, and unsupported prompt conclusions.
- Produce a self-contained native runtime with no Python, `uv`, or virtual-environment dependency after cutover.
- Measure cold start, resident memory, and local wrapper overhead separately from Tiingo network latency.
- Preserve a clean path to later REST modules and bounded Tokio WebSocket tasks without implementing them now.

## Non-goals

- No new Tiingo endpoint family in this delivery.
- No Streamable HTTP transport in this delivery; stdio remains the only shipped transport until a hosted deployment is designed.
- No unbounded event streaming or background market-data ingestion.
- No Python or PyPI compatibility shim. In particular, `uvx tiingo-mcp` ends at the Rust major release.
- No generated Tiingo client. Tiingo does not publish an authoritative OpenAPI document suitable for code generation.
- No performance claim based on upstream request duration; Tiingo latency and quotas are external to the runtime.

## User-facing launch contract

The production executable owns RMCP's stdio transport. Running the binary with no arguments blocks on stdin/stdout until the MCP host closes the session:

```text
tiingo-mcp
```

The normal MCP configuration is name-based:

```json
{
  "mcpServers": {
    "tiingo": {
      "command": "tiingo-mcp",
      "args": [],
      "env": {
        "TIINGO_API_KEY": "..."
      }
    }
  }
}
```

There is no `rmcp run`, `cargo run`, shell wrapper, or package-manager command in the production launch chain. RMCP's `transport-io` feature provides the server's stdin/stdout transport. All tracing and diagnostics go to stderr; stdout is reserved exclusively for MCP JSON-RPC messages.

The installer must put `tiingo-mcp` on `PATH`, and the documentation treats an absolute `command` path only as a troubleshooting fallback. GUI hosts that do not reliably inherit a shell `PATH` receive a self-contained MCPB binary bundle. The bundle manifest resolves its internal executable, so the user does not edit or even see an absolute path.

The binary also supports `--help` and `--version`. Those flags print and exit; invocation without a flag always starts stdio. No `serve` subcommand is added because it would make the common MCP configuration noisier without adding value.

## Transport decision

The `TiingoServer` handler and client are transport-independent, but `2.0.0` wires them only to RMCP stdio. This is the right boundary for the parity release because the MCP host owns the child process and its lifetime, credentials stay in the child environment, no port is exposed, and there is no separate daemon to install or supervise.

Streamable HTTP is applicable in two later deployment shapes:

| Shape | MCP configuration | Benefit | Required design work |
|---|---|---|---|
| Local HTTP daemon | `http://127.0.0.1:<port>/mcp` | Several local clients can share one process | Process supervision, port selection, localhost binding, Origin validation, and client authentication |
| Hosted HTTP service | `https://<host>/mcp` | No local binary or executable path; cross-device access | Hosting, TLS, OAuth resource-server behavior, tenant isolation, quotas, and secure Tiingo credential custody |

Current MCP Streamable HTTP is POST-based and stateless at the protocol level; the 2026-07-28 revision removed the older GET stream and protocol session identifier. Any later stateful Tiingo feed therefore returns an explicit stream handle from a tool rather than hiding ownership in transport state.

Tiingo WebSockets are upstream data sources and do not require HTTP as the downstream MCP transport. A stdio RMCP process can own Tokio WebSocket tasks, cancellation, and bounded buffers. HTTP becomes useful when a concrete shared or hosted service needs those tasks to outlive one local client process.

The project will not implement legacy HTTP+SSE. A later HTTP specification must choose local-only or hosted operation before code is written; those modes have materially different authentication and secret-management requirements.

## Compatibility contract

### Tools

The following names and their existing exposed argument names, required fields, optional fields, and defaults remain compatible:

1. `get_stock_metadata`
2. `get_stock_prices`
3. `get_realtime_price`
4. `get_intraday_prices`
5. `get_forex_quote`
6. `get_forex_prices`
7. `get_crypto_quote`
8. `get_crypto_prices`
9. `get_crypto_metadata`
10. `get_news`
11. `get_fundamentals_definitions`
12. `get_financial_statements`
13. `get_daily_fundamentals`
14. `get_company_meta`
15. `get_dividends`
16. `get_dividend_yield`
17. `get_splits`

### Resources and prompts

The fixed resource URIs remain:

- `tiingo://capabilities`
- `tiingo://fundamentals/definitions`
- `tiingo://guide/date-formats`

The `tiingo://guide/{asset_class}` resource template and its six current asset-class values remain discoverable. The prompt names remain `analyze-stock`, `compare-stocks`, `crypto-market-overview`, `earnings-report-analysis`, and `forex-pair-analysis`, with compatible arguments and defaults.

### Results

Successful tools retain a JSON text content block so existing consumers can continue parsing the result as they do today. The Rust result also exposes structured content in the additive object form `{ "data": ..., "meta": ... }`; an upstream top-level array is placed under `data` rather than forced into an inaccurate response model. Unknown Tiingo fields are preserved with `serde_json::Value`.

Recoverable tool failures intentionally correct the current contract: they set MCP `isError: true` and retain a concise JSON text error block for older clients. No raw `reqwest` error, authorization header, or API key reaches the client.

## Approved correctness deltas from the Python baseline

The migration freezes the current contract before implementation, but it does not reproduce these known defects:

- `get_crypto_quote` uses Tiingo's current prices route rather than the deprecated broad top-of-book route while retaining its MCP name and inputs.
- Capabilities and guide resources stop presenting changing plan names and rate limits as timeless facts. Entitlement statements are source-dated and use capability language.
- Share-class examples use Tiingo dash symbology such as `BRK-A`.
- The server description no longer claims the entire Tiingo API is already exposed.
- `analyze-stock` requests company metadata before asking for sector; the earnings prompt does not label a result a beat or miss without consensus data; the forex prompt does not assign causes from price history alone.
- Tool failures use MCP error semantics instead of successful JSON text containing an `error` key.
- The HTTP client has an explicit bounded retry policy rather than describing connect-only transport retries as general retries.

These deltas are versioned contract fixtures and reviewed as intentional differences. All other unexplained drift is a migration failure.

## Target code layout

The repository becomes one Cargo package rather than a workspace with a single member:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
src/
  main.rs                 executable, CLI flags, stderr tracing, stdio lifecycle
  lib.rs                  testable server construction
  config.rs               environment and fixed runtime limits
  error.rs                TiingoError classification and MCP mapping
  client/
    mod.rs                reqwest client, auth, timeout, retry, decoding
    eod.rs
    iex.rs
    forex.rs
    crypto.rs
    news.rs
    fundamentals.rs
    corporate_actions.rs
  mcp/
    mod.rs                TiingoServer and RMCP capability declaration
    tools.rs              stable tool router
    resources.rs          fixed resources and guide template
    prompts.rs            prompt router and corrected content
tests/
  contract/               canonical MCP discovery and call snapshots
  fixtures/               Tiingo request/response fixtures
packaging/
  mcpb/                   binary-bundle manifest and packaging inputs
```

The family modules are the only extension seams added in advance: the approved follow-on API surface already spans these families. No generic endpoint registry, plugin system, or code-generation layer is introduced.

## Runtime components and data flow

1. The MCP host spawns `tiingo-mcp` and passes `TIINGO_API_KEY` in the child environment.
2. `main` configures stderr-only tracing, constructs `TiingoServer`, calls RMCP `serve(stdio())`, and waits for shutdown.
3. RMCP validates a tool request against a `schemars` schema generated from a typed parameter struct.
4. The tool router normalizes only documented inputs and calls the appropriate family method on one shared `reqwest::Client`.
5. The client adds `Authorization: Token ...`, applies timeout and retry policy, decodes the upstream body as JSON, and preserves unmodeled fields.
6. The tool maps success to the compatibility text block and additive structured content, or maps `TiingoError` to `isError: true`.
7. Closing stdin ends the RMCP service and drops the shared HTTP client without a background process.

The API key may be loaded lazily on the first Tiingo tool call so discovery, resources, and prompts remain usable without credentials. A missing key produces a classified tool error rather than crashing an otherwise valid MCP initialization.

## Types and validation

- Request arguments use dedicated `serde`/`schemars` structs and small enums for closed choices such as resample frequency and news sort order.
- Ticker fields remain strings where Tiingo has product-specific formats; lightweight validation rejects empty values and unsafe path characters without pretending to own Tiingo's evolving symbol catalog.
- Dates are validated as `YYYY-MM-DD` before a request is consumed.
- Query construction uses `reqwest` serialization rather than string concatenation.
- Responses remain flexible JSON values inside a stable result envelope. Strictly modeling every upstream response field would turn harmless Tiingo additions into decode failures.

## Error and retry policy

`TiingoError` distinguishes configuration, validation, authentication, entitlement, not found, rate limit, transient upstream, timeout, transport, and decode failures.

All Tiingo operations in this delivery are GET requests. A call is attempted at most three times. Retries are limited to connection/read timeouts, HTTP 429, and HTTP 502/503/504. The client respects `Retry-After` when valid and otherwise uses capped jittered backoff. It never retries validation failures or HTTP 400/401/403/404.

Error text includes the Tiingo capability and a useful next action but does not speculate about a user's subscription plan. Logs redact the authorization header and never serialize the API key. Unexpected responses record status and a bounded body excerpt on stderr; the MCP error receives a sanitized message.

The client enforces an 8 MiB maximum decoded response size in this release. Crossing it returns a classified error that asks the caller to narrow dates, tickers, or limits. The limit is a tested constant, not exposed as premature configuration; boundary tests cover decoded upstream responses, while the performance report records the frozen MCP contract fixture only as local payload context.

## Testing and evidence

### Baseline capture

Before Rust source is added, the Python server is used to capture canonical JSON fixtures for:

- legacy `initialize`, protocol negotiation, `tools/list`, `resources/list`, `resources/templates/list`, and `prompts/list`; the Python baseline also records its `-32602` rejection of current `server/discover`, while Rust must support that current discovery method;
- every tool's input schema and representative successful request mapping;
- every fixed resource, every guide template value, and every prompt/default combination;
- representative 401, 403, 404, 429, timeout, malformed JSON, and 5xx behavior.

Canonicalization ignores object key order and records the approved correctness deltas explicitly.

### Rust verification

- Unit tests cover validation, parameter serialization, retry classification, backoff bounds, redaction, and error mapping.
- Fixture tests assert the exact HTTP method, path, query, and authorization behavior of all 17 tools.
- RMCP in-process and child-process tests assert supported protocol negotiation, discovery, calls, resources, templates, prompts, clean shutdown, and stderr/stdout separation.
- Contract tests compare the canonical Rust output to the frozen baseline plus the approved deltas.
- A quota-bounded live smoke uses representative public endpoints when `TIINGO_API_KEY` is available. Entitlement failures are reported separately from implementation failures.
- CI runs format, Clippy with warnings denied, every offline test, a release build, and dependency/license auditing.

### Performance evidence

The final migration report records:

- median and p95 time from process spawn to successful MCP initialization over at least 30 cold starts;
- resident memory after initialization and after the same fixture workload;
- median and p95 wrapper overhead against a local HTTP fixture server;
- the same measurements from the frozen Python baseline on the same machine.

Rust must improve cold start and resident memory and must not regress local wrapper p95. Tiingo network time is reported separately and is not attributed to Rust.

## Distribution

The cutover is released as `2.0.0` because it intentionally removes the PyPI/`uvx` install contract and corrects error semantics.

Release automation builds `tiingo-mcp` for Apple Silicon and Intel macOS, x86-64 and ARM64 Linux, and x86-64 Windows. Linux artifacts use Rustls and a portable static target where dependencies permit it. Tagged GitHub releases contain archives, SHA-256 checksums, and immutable source references. `cargo install tiingo-mcp` is supported after the crate name and publishing route are verified; GitHub binaries remain the runtime-independent distribution.

Two MCP-facing installation paths are documented:

1. Install the binary on `PATH`, verify it with the platform's command lookup, and use `command: "tiingo-mcp"` with no args.
2. Install the matching MCPB binary bundle in a supporting desktop host or through Smithery, with the API key collected as sensitive user configuration.

The current Smithery `uvx` command function is removed. It is replaced by the MCPB publication for local zero-configuration use; a plain name-based stdio stanza remains available for hosts that consume registry launch metadata.

## Cutover sequence

1. Freeze the Python MCP and HTTP contract and record the intentional corrections.
2. Build the Rust server alongside Python only for comparison on the migration branch.
3. Pass offline contract, child-process, error, audit, performance, and authorized live-smoke gates.
4. Replace CI, release automation, Smithery/MCPB metadata, README examples, badges, changelog, and repository guidance with their Rust equivalents.
5. Delete `src/tiingo_mcp`, all Python tests, `pyproject.toml`, `uv.lock`, Python-specific configuration, and the local virtual environment.
6. Prove a clean clone can build, test, package, install, and launch using only the declared Rust toolchain or a downloaded native artifact.
7. After an explicit release-readiness handoff and user authorization, create and push the `2.0.0` tag, publish the crate, create the GitHub release, and publish the MCPB bundles. The last Python release remains recoverable from its Git tag and Git history but is not retained on the new branch. For this release, that authorization was granted while the PR remained open.

Python deletion happens only after step 3. The virtual environment is untracked and is removed last; it can be recreated from the historical release but is not recoverable in place.

## Acceptance criteria

1. A clean MCP host launches the installed server with `command: "tiingo-mcp"` and no args.
2. A supporting desktop host installs the platform MCPB without a user-entered executable path.
3. All 17 legacy tools, three fixed resources, one guide template, and five prompts remain discoverable under their existing identifiers and compatible schemas.
4. Every tool has an exact route and parameter test, and all approved correctness deltas are explicit fixtures.
5. Errors are classified, set `isError: true`, respect bounded retry rules, and never expose credentials.
6. stdout contains only MCP protocol traffic; diagnostics are visible on stderr.
7. Full offline CI, release build, audit, child-process contract tests, and the authorized live smoke pass.
8. Measured cold start and resident memory improve over the Python baseline, and local wrapper p95 does not regress.
9. Supported release artifacts install and run on clean target systems with checksums published.
10. No Python source, Python packaging, `uv` lockfile, or virtual environment remains after the verified cutover.

## Follow-on boundaries

After `2.0.0` is accepted, REST expansion receives its own design and implementation plan. That work will add result pagination/row caps, remaining documented parameters, asset search, Equity Realtime, BOATS, batch corporate actions, and entitlement-gated fund data. WebSockets receive a third design centered on bounded event counts or duration, Tokio cancellation, backpressure, reconnects, dropped-event metrics, and MCP task/resource subscription compatibility.

## Primary references

- [Official RMCP Rust SDK and stdio transport](https://github.com/modelcontextprotocol/rust-sdk)
- [MCP local stdio server configuration](https://modelcontextprotocol.io/docs/2026-07-28/develop/connect-local-servers)
- [MCP Streamable HTTP transport](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP HTTP authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- [MCPB binary manifest specification](https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md)
- [Smithery local MCPB publishing](https://smithery.ai/docs/build/publish)
- [Cargo binary installation](https://doc.rust-lang.org/cargo/commands/cargo-install.html)
- [Migration and Tiingo API research report](../../../exa-results/tiingo-rust-rmcp-api-expansion-report-2026-08-24.md)
