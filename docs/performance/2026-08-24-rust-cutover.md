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

RSS is the sum of the complete launched process tree: the direct command PID plus every recursively discovered descendant. This deliberately includes both the resident `uv` launcher and its Python server descendant for the Python command; the Rust command has only its server process. The same tree definition is applied with sysinfo after initialization and after the full workload. On every success or error path, the probe closes stdin, preserves any original probe error, waits at most five seconds, and force-terminates the tracked descendants and direct child before reaping the child if the tree does not exit. Each runtime was measured for exactly 30 runs. After sorting, the deterministic zero-based indices are 14 for the median (`(30 - 1) / 2`) and 28 for p95 (`ceil(0.95 * 30) - 1`).

The wrapper probes each perform 10 warmup calls followed by exactly 1,000 measured `get_stock_metadata(AAPL)` calls. Setup, warmup, and teardown are outside the samples. The deterministic zero-based indices are 499 for the median and 949 for p95. Python uses the prescribed `httpx.MockTransport` with a `fixture.invalid` hostname assertion. Rust constructs `TiingoClient` directly with an internal Wiremock loopback URL, does not read `TIINGO_API_KEY`, and verifies that Wiremock captured exactly 1,010 requests. No wrapper probe can reach Tiingo.

The managed execution sandbox denied access to uv's existing cache and denied Wiremock loopback binding. The affected local-only commands were therefore run through the approved unsandboxed execution path on the same machine. The corrected Python and Rust 30-run process measurements were both run through that same unsandboxed path so their environment was identical. This is an execution-environment distinction, not a product failure.

## Raw outputs

```text
$ cargo build --release
    Finished `release` profile [optimized] target(s) in 0.14s

$ cargo run --release --example migration_probe -- --runs 30 -- uv run tiingo-mcp
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/examples/migration_probe --runs 30 -- uv run tiingo-mcp`
{"rss_initialized_bytes_median":113721344,"rss_workload_bytes_median":114032640,"runs":30,"startup_ms_median":478.661958,"startup_ms_p95":598.049666}

$ cargo run --release --example migration_probe -- --runs 30 -- target/release/tiingo-mcp
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/examples/migration_probe --runs 30 -- target/release/tiingo-mcp`
{"rss_initialized_bytes_median":10305536,"rss_workload_bytes_median":10797056,"runs":30,"startup_ms_median":4.8952919999999995,"startup_ms_p95":5.304125}

$ uv run python scripts/benchmark_python_wrapper.py
{"runs": 1000, "wrapper_us_median": 848.209, "wrapper_us_p95": 1008.917}

$ cargo run --release --example migration_probe -- --wrapper-runs 1000
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/examples/migration_probe --wrapper-runs 1000`
{"runs":1000,"wrapper_us_median":79.667,"wrapper_us_p95":157.917}
```

## Gate calculations

Percentages are reductions from Python: `(Python - Rust) / Python * 100`.

| Predicate | Python | Rust | Difference | Result |
|---|---:|---:|---:|---|
| Median cold start | 478.661958 ms | 4.895292 ms | 98.977297% lower | PASS |
| p95 cold start | 598.049666 ms | 5.304125 ms | 99.113096% lower | informational |
| Median initialized RSS | 113,721,344 bytes | 10,305,536 bytes | 90.937905% lower | PASS |
| Median post-workload RSS | 114,032,640 bytes | 10,797,056 bytes | 90.531609% lower | PASS |
| Median local wrapper | 848.209 us | 79.667 us | 90.607621% lower | informational |
| p95 local wrapper | 1,008.917 us | 157.917 us | 84.347870% lower | PASS |

All four required predicates pass. Python remains present until the separately gated deletion task.

## Response-body cap margin and network exclusion

The largest captured fixture is `tests/contract/baseline/python-mcp.json` at 52,788 bytes. Against the 8 MiB decoded-body cap of 8,388,608 bytes, the remaining margin is 8,335,820 bytes, or 99.370718% of the cap.

Tiingo network latency was excluded from every measurement. The stdio workload uses only discovery, fixed resources, and prompt rendering and makes no tool calls. The wrapper workload uses only the fixed local/mock HTTP responders described above. These figures measure process/runtime and local wrapper behavior, not Tiingo service latency.
