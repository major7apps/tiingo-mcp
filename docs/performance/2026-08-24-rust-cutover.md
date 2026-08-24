# Rust cutover performance evidence

Date: 2026-08-24

Outcome: PASS. On the same machine, Rust had a lower median cold start, lower median initialized RSS, lower median post-workload RSS, and a lower local-wrapper p95 than the frozen Python server.

## Machine and tool versions

- Machine: Mac14,13, Apple M2 Max, 68,719,476,736 bytes RAM, arm64
- OS: macOS 26.6.2 (build 25G83)
- Rust: rustc 1.88.0 (6b00bc388 2025-06-23), aarch64-apple-darwin, LLVM 20.1.5
- Cargo: 1.88.0 (873a06493 2025-05-10)
- RMCP: 3.1.4
- sysinfo: 0.37.2
- Wiremock: 0.6.5
- uv: 0.11.29 (901092ee1 2026-07-15 aarch64-apple-darwin)
- Python: 3.13.11
- FastMCP: 3.2.3
- httpx: 0.28.1
- frozen Python `tiingo-mcp`: 1.1.0

## Method

The process probe starts a fresh child for every run and measures from immediately before process spawn through the first complete, successful MCP `initialize` response using protocol `2025-06-18`. After initialization, both runtimes receive the same real newline-delimited stdio MCP workload, in this order:

1. `tools/list`
2. `resources/list`
3. `resources/read` for `tiingo://capabilities`
4. `resources/read` for `tiingo://fundamentals/definitions`
5. `resources/read` for `tiingo://guide/date-formats`
6. `prompts/get` for `analyze-stock` with `ticker=AAPL` and the default `include_news`
7. `prompts/get` for `compare-stocks` with `ticker1=AAPL`, `ticker2=MSFT`, and the default period
8. `prompts/get` for `crypto-market-overview` with default tickers
9. `prompts/get` for `earnings-report-analysis` with `ticker=NVDA` and `earnings_date=2024-02-21`
10. `prompts/get` for `forex-pair-analysis` with `pair=eurusd` and the default period

RSS is the sum of the complete launched process tree: the direct command PID plus every recursively discovered descendant. This deliberately includes both the resident `uv` launcher and its Python server descendant for the Python command; the Rust command has only its server process. The same tree definition is applied with sysinfo after initialization and after the full workload. Unix commands are placed in a dedicated process group at spawn; Windows uses the platform's narrow tree-termination fallback without adding a dependency. On every success or error path, the probe closes stdin, preserves any original probe error, waits at most five seconds, and terminates the OS-owned containment before reaping the direct child if the tree does not exit. Each runtime was measured for exactly 30 runs. After sorting, the median is the average of zero-based indices 14 and 15; p95 is index 28 (`ceil(0.95 * 30) - 1`).

The wrapper probes each perform 10 warmup calls followed by exactly 1,000 measured `get_stock_metadata(AAPL)` calls. Setup, warmup, and teardown are outside the samples. The median is the average of zero-based indices 499 and 500; p95 is index 949. Both clients use their normal HTTP stacks against bounded responders on `127.0.0.1`: Python uses a temporary standard-library threaded HTTP/1.1 fixture with persistent connections, and Rust uses Wiremock. Both probes verify exactly 1,010 authenticated requests. Neither reads the ambient `TIINGO_API_KEY`, and neither can reach Tiingo.

The managed execution sandbox denied access to uv's existing cache and denied Wiremock loopback binding. The affected local-only commands were therefore run through the approved unsandboxed execution path on the same machine. The corrected Python and Rust 30-run process measurements were both run through that same unsandboxed path so their environment was identical. This is an execution-environment distinction, not a product failure.

## Raw outputs

```text
$ cargo build --release
    Finished `release` profile [optimized] target(s) in 0.14s

$ UV_PROJECT_ENVIRONMENT=<temporary> cargo run --release --example migration_probe -- --runs 30 -- uv run --locked --project <frozen-python-checkout> tiingo-mcp
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/examples/migration_probe --runs 30 -- uv run --locked --project <frozen-python-checkout> tiingo-mcp`
{"rss_initialized_bytes_median":113360896,"rss_workload_bytes_median":113664000,"runs":30,"startup_ms_median":441.81962450000003,"startup_ms_p95":523.142541}

$ cargo run --release --example migration_probe -- --runs 30 -- target/release/tiingo-mcp
    Finished `release` profile [optimized] target(s) in 0.12s
     Running `target/release/examples/migration_probe --runs 30 -- target/release/tiingo-mcp`
{"rss_initialized_bytes_median":10280960,"rss_workload_bytes_median":10797056,"runs":30,"startup_ms_median":4.369249999999999,"startup_ms_p95":5.579416999999999}

$ UV_PROJECT_ENVIRONMENT=<temporary> uv run --locked --project <frozen-python-checkout> python <temporary-loopback-wrapper-probe>
{"runs": 1000, "wrapper_us_median": 1526.5835000000002, "wrapper_us_p95": 1821.5}

$ cargo run --release --example migration_probe -- --wrapper-runs 1000
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/examples/migration_probe --wrapper-runs 1000`
{"runs":1000,"wrapper_us_median":80.9375,"wrapper_us_p95":150.416}
```

## Gate calculations

Percentages are reductions from Python: `(Python - Rust) / Python * 100`.

| Predicate | Python | Rust | Difference | Result |
|---|---:|---:|---:|---|
| Median cold start | 441.819625 ms | 4.369250 ms | 99.011078% lower | PASS |
| p95 cold start | 523.142541 ms | 5.579417 ms | 98.933481% lower | informational |
| Median initialized RSS | 113,360,896 bytes | 10,280,960 bytes | 90.930770% lower | PASS |
| Median post-workload RSS | 113,664,000 bytes | 10,797,056 bytes | 90.500901% lower | PASS |
| Median local wrapper | 1,526.5835 us | 80.9375 us | 94.698128% lower | informational |
| p95 local wrapper | 1,821.500 us | 150.416 us | 91.742191% lower | PASS |

All four required predicates pass. Python was then removed by the separately gated deletion task; the historical checkout was used only in a temporary environment for this corrected comparison.

## Contract-payload margin and network exclusion

The largest frozen MCP contract fixture is `tests/contract/baseline/python-mcp.json` at 52,788 bytes. Against the 8 MiB decoded-body cap of 8,388,608 bytes, the remaining local contract-payload margin is 8,335,820 bytes, or 99.370718% of the cap. This fixture is not evidence about Tiingo response sizes; the upstream decoded-body limit is independently exercised at and beyond the boundary in `tests/client_http.rs`.

Tiingo network latency was excluded from every measurement. The stdio workload uses only discovery, fixed resources, and prompt rendering and makes no tool calls. The wrapper workload uses only the fixed local/mock HTTP responders described above. These figures measure process/runtime and local wrapper behavior, not Tiingo service latency.
