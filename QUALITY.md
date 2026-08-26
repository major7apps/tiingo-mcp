# Quality gates

Release quality means the exact committed head passes deterministic local gates, the matching CI head passes both toolchains and all native targets, and no quota-consuming test is inferred from offline evidence.

## Local release gate

Run from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo llvm-cov --all-targets --all-features --locked --fail-under-lines 93 --summary-only
cargo build --release --locked
cargo deny check all
dist plan
```

`cargo run --quiet -- --version` is the local CLI smoke. The dependency and build commands stay locked. A logged network, protocol, or build error is a failure even if its command exits zero.

## CI contract

The Rust matrix runs 1.88.0 (the MSRV) and stable. Both run strict Clippy and release builds. MSRV runs the full locked offline suite; stable additionally runs formatting, the full locked offline suite under `cargo llvm-cov`, a 93% line-coverage floor, and `cargo deny check all`.

Native artifact jobs build and validate the binary and MCPB bundle on:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-musl`
- `x86_64-unknown-linux-musl`
- `x86_64-pc-windows-msvc`

Release preparation also requires `dist plan`. A pull request is ready only when CodeRabbit has given an actual approval and every required check, including all five native/MCPB jobs, is green on that exact head.

## Contract and direct-test expectations

- `tests/client_http.rs` exercises authentication, retries, redaction, same-origin enforcement, decoded response bounds, and JSON/CSV behavior.
- `tests/client_market_routes.rs` and `tests/client_data_routes.rs` assert literal Tiingo paths and queries for every REST family.
- `tests/mcp_tools.rs` calls the real in-memory RMCP boundary and checks JSON text, structured data, validation, consistency, and deterministic latency.
- `tests/websocket_protocol.rs` covers official message/frame shapes and malformed or unknown input.
- `tests/websocket_lifecycle.rs` covers validation, queue limits, cursor replay, data gaps, reconnects, liveness, expiry, redaction, cancellation, and worker cleanup with controlled clocks/sockets.
- `tests/websocket_logging.rs` runs a child process against a concrete mock socket with dependency trace logging requested and proves credentials and upstream subscription IDs never reach stdout or stderr.
- `tests/mcp_resources.rs`, `tests/mcp_prompts.rs`, and `tests/project_docs.rs` enforce reference validity and the public resource/document surface.
- `tests/stdio_process.rs` proves initialization, cancellation, EOF shutdown, and stdout purity in a child process.

`tests/contract/baseline/v1-mcp.json` is the immutable 17-tool parity oracle. `tests/mcp_contract.rs` applies only named correctness/additive deltas, verifies that the original 17 descriptors remain compatible, and separately requires 38 tools, three fixed resources, one resource template, and five prompts.

Every new REST route/query, MCP tool call, WebSocket frame/parser branch, and lifecycle transition begins with a deterministic failing test. New client, MCP, and runtime code receives direct coverage; aggregate coverage alone is insufficient.

## Offline and live evidence

The normal suite is deterministic, local, and credential-free. Mock HTTP servers and controlled WebSocket connectors provide exact response, timing, failure, and entitlement fixtures.

Ignored live tests are read-only, require a nonempty `TIINGO_API_KEY`, and consume quota or bandwidth. Run them only with explicit authorization. Use fixed tickers, dates, small filters, and finite waits. Treat a documented 403 as entitlement evidence, not a product failure. Never use bulk or all-market operations as routine smoke tests, and never force reconnect/flood behavior against Tiingo.

`L` in [API_SURFACE.md](API_SURFACE.md) means bounded read-only live-testable, not that each operation has a dedicated ignored case. The separate checked-in live-smoke inventory is the exact harness; its test names and represented tools are mechanically reconciled with the Rust test sources.

Fixture accuracy means literal fields survive the Tiingo-to-MCP boundary and documented invariants hold. A live response from Tiingo is consistency/shape evidence, not independent price accuracy. Any independent-price accuracy claim requires a separately sourced, contemporaneous comparison with the source and observation time recorded.

## Performance evidence

Warm deterministic paths before sampling. Report a latency distribution as `count`, `min`, `p50`, `p95`, and `max`, with the operation, fixture/live class, and timeout or threshold. Do not present a single timing as a distribution or compare differently scoped process trees/workloads.

## Credential and protocol safety

Tests must prove that API keys, authorization headers, upstream subscription IDs, and token-bearing error details never appear in debug output, dependency trace logs, retained events, MCP text, structured content, or stdout. Only MCP protocol bytes go to stdout; diagnostics go to stderr. Recoverable failures keep sanitized legacy JSON text and set MCP `isError: true`; terminal WebSocket state exposes only a sanitized classification.

## Documentation freshness

Changes to registered tools, routes, access language, lifecycle bounds, commands, CI, resources, templates, or prompts update the matching root references in the same change. `AGENTS.md` remains a map. `tests/project_docs.rs` resolves root links, preserves the `CLAUDE.md` symlink, parses embedded JSON, checks guide tool names against real discovery, and requires each discovered tool exactly once in the README and [API_SURFACE.md](API_SURFACE.md) tool tables.

Entitlement and beta statements are source-dated. Review [API_SURFACE.md](API_SURFACE.md), capabilities, and guides against official Tiingo sources when behavior changes or before release; do not promise plan-tier access from an old observation.
