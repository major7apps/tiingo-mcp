# Tiingo MCP Rust/RMCP Parity and Python Cutover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Python/FastMCP server with a verified Rust 2.0.0 binary that preserves the 17-tool, three-resource, one-template, five-prompt contract, corrects the approved defects, ships native artifacts and MCPB bundles, and removes Python from the repository.

**Architecture:** One root Cargo package exposes a transport-independent `TiingoServer` backed by one shared `reqwest::Client`; RMCP stdio is the only 2.0.0 transport. Typed request structs feed family-specific route modules, flexible `serde_json::Value` responses preserve upstream fields, and one error layer maps Tiingo failures to MCP `isError: true` results. Python remains only long enough to capture and compare the baseline, then is deleted after all Rust gates pass.

**Tech Stack:** Rust 1.88+, edition 2024, RMCP 3.1.4, Tokio, reqwest 0.13.4 with Rustls, serde/schemars, clap, tracing, wiremock, cargo-deny, cargo-dist 0.32.0, MCPB CLI 2.1.2.

**Spec:** `docs/superpowers/specs/2026-08-24-tiingo-rust-rmcp-parity-cutover-design.md`

## Global Constraints

- Read the spec before Task 1 and keep it open during every review.
- Work on a dedicated implementation branch or worktree created at execution time; do not implement directly on `main`.
- The Cargo package and executable are both named `tiingo-mcp`; the release version is `2.0.0`.
- Set `edition = "2024"`, `rust-version = "1.88"`, and pin RMCP exactly to `=3.1.4`.
- `tiingo-mcp` with no arguments starts RMCP stdio. `--help` and `--version` print and exit. No `serve` subcommand is added.
- stdout is MCP protocol only. All tracing and diagnostics use stderr.
- Keep `TIINGO_API_KEY`; load it lazily enough that discovery, resources, and prompts work without a key.
- Keep all 17 tool names, existing exposed argument names/defaults, `tiingo://capabilities`, `tiingo://fundamentals/definitions`, `tiingo://guide/date-formats`, `tiingo://guide/{asset_class}`, and all five prompt names.
- Successful tools return the legacy pretty JSON text block plus structured content shaped as `{ "data": value, "meta": { "source": "tiingo" } }`.
- Tool execution failures return `isError: true` with sanitized text and structured error content. Credentials and raw authorization headers never enter logs or results.
- Use at most three GET attempts. Retry only connect/read timeouts, 429, 502, 503, and 504; honor `Retry-After`; never retry 400, 401, 403, or 404.
- Cap decoded upstream bodies at 8 MiB and error before JSON decoding when the cap is exceeded.
- Fix only the approved correctness deltas: current crypto prices route, source-dated entitlement facts, `BRK-A`, truthful coverage text, corrected prompts, real MCP errors, and explicit retry behavior.
- Do not add Streamable HTTP, new Tiingo product families, WebSocket behavior, a Python/PyPI shim, or generic plugin/code-generation layers.
- Do not delete any Python file or `.venv` until Tasks 1–11 pass, including all five clean-target CI jobs. The final deletion is Task 12.
- Build release targets for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-musl`, `x86_64-unknown-linux-musl`, and `x86_64-pc-windows-msvc`.
- Do not create or push a release tag during implementation. Report release readiness and request separate release authorization.

---

## File Map

| Path | Responsibility |
|---|---|
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | Root Rust package, locked dependencies, MSRV toolchain |
| `src/main.rs` | CLI parsing, stderr tracing, stdio process lifecycle |
| `src/lib.rs` | Public module boundary and `run_stdio` entrypoint |
| `src/config.rs` | Environment loading, base URL, timeouts, retry constants, 8 MiB cap |
| `src/error.rs` | `TiingoError`, sanitized payloads, status classification |
| `src/client/mod.rs` | Shared reqwest client, authenticated bounded GET, retry loop |
| `src/client/query.rs` | Shared serialized date/resample query types |
| `src/client/{eod,iex,forex,crypto,news,fundamentals,corporate_actions}.rs` | Exact Tiingo route and query mappings |
| `src/mcp/mod.rs` | `TiingoServer`, RMCP `ServerHandler`, advertised capabilities |
| `src/mcp/tools.rs` | Typed tool inputs, 17 RMCP routes, result conversion |
| `src/mcp/resources.rs`, `src/mcp/data/**` | Three fixed resources, guide template, corrected static JSON |
| `src/mcp/prompts.rs` | Five prompt definitions, argument parsing, corrected messages |
| `tests/contract/baseline/python-mcp.json` | Frozen Python protocol, discovery, HTTP mapping, error, resource, and prompt contract |
| `tests/client_*.rs` | Route, query, retry, body-cap, and secret tests |
| `tests/mcp_contract.rs`, `tests/stdio_process.rs`, `tests/live_smoke.rs` | RMCP parity, real child process, optional live evidence |
| `examples/migration_probe.rs` | Repeatable Python-versus-Rust cold-start/RSS measurement |
| `docs/performance/2026-08-24-rust-cutover.md` | Recorded performance evidence |
| `.github/workflows/ci.yml`, `.github/workflows/release.yml` | Full Rust CI and generated cargo-dist release workflow |
| `dist-workspace.toml` | cargo-dist 0.32.0 targets, installers, checksums, attestations |
| `packaging/mcpb/manifest.json`, `packaging/mcpb/package.sh` | Path-free binary bundle metadata and deterministic packaging |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `.gitignore` | Rust installation, launch, development, release, repository guidance |
| `scripts/capture_python_contract.py`, `tests/test_contract_baseline.py` | Temporary Python baseline capture; removed in Task 12 |
| `scripts/export_rust_resources.py` | Temporary deterministic resource converter; removed in Task 12 |

---

### Task 1: Freeze the Python contract before changing runtime code

**Files:**
- Create: `scripts/capture_python_contract.py`
- Create: `tests/test_contract_baseline.py`
- Generate: `tests/contract/baseline/python-mcp.json`
- Verify: existing `src/tiingo_mcp/**` and `tests/test_*.py`

**Interfaces:**
- Consumes: current FastMCP `mcp` object and the 1.1.0 Python tests.
- Produces: canonical `python-mcp.json` with protocol negotiation, all 17 HTTP request mappings, representative errors, tools, resources, templates, resource contents, prompts, and prompt results for Tasks 4–9.

- [ ] **Step 1: Verify the unchanged Python baseline**

Run:

```bash
uv run ruff check src/ tests/
uv run ruff format --check src/ tests/
uv run pytest -v
```

Expected: Ruff passes; pytest reports `137 passed, 4 skipped` when the same entitlement-limited credential is present, or the live module skips when no key is present.

- [ ] **Step 2: Write the failing baseline completeness test**

Create `tests/test_contract_baseline.py`:

```python
from __future__ import annotations

import json
from pathlib import Path

BASELINE = Path("tests/contract/baseline/python-mcp.json")


def test_python_contract_baseline_is_complete() -> None:
    assert BASELINE.exists()
    data = json.loads(BASELINE.read_text())
    assert data["initialize"]["protocolVersion"]
    assert data["server_discover"]["error"]["code"] == -32602
    assert len(data["http_requests"]) == 17
    assert set(data["errors"]) == {"401", "403", "404", "429", "500", "malformed_json", "timeout"}
    assert sorted(tool["name"] for tool in data["tools"]) == sorted(
        [
            "get_stock_metadata", "get_stock_prices", "get_realtime_price",
            "get_intraday_prices", "get_forex_quote", "get_forex_prices",
            "get_crypto_quote", "get_crypto_prices", "get_crypto_metadata",
            "get_news", "get_fundamentals_definitions", "get_financial_statements",
            "get_daily_fundamentals", "get_company_meta", "get_dividends",
            "get_dividend_yield", "get_splits",
        ]
    )
    assert len(data["resources"]) == 3
    assert len(data["resource_templates"]) == 1
    assert len(data["prompts"]) == 5
    assert set(data["resource_contents"]) == {
        "tiingo://capabilities",
        "tiingo://fundamentals/definitions",
        "tiingo://guide/date-formats",
        "tiingo://guide/corporate-actions",
        "tiingo://guide/crypto",
        "tiingo://guide/forex",
        "tiingo://guide/fundamentals",
        "tiingo://guide/news",
        "tiingo://guide/stocks",
    }
```

- [ ] **Step 3: Run the test and confirm the missing fixture failure**

Run: `uv run pytest tests/test_contract_baseline.py -v`

Expected: FAIL at `assert BASELINE.exists()`.

- [ ] **Step 4: Add the deterministic capture script**

Create `scripts/capture_python_contract.py`:

```python
from __future__ import annotations

import asyncio
import json
from collections.abc import Callable
from pathlib import Path
from typing import Any, Literal

import httpx
from fastmcp import Client
from mcp import McpError, types as mcp_types

from tiingo_mcp.client import TiingoClient
from tiingo_mcp.server import mcp

OUTPUT = Path("tests/contract/baseline/python-mcp.json")
GUIDES = ["corporate-actions", "crypto", "forex", "fundamentals", "news", "stocks"]
FIXED_RESOURCES = [
    "tiingo://capabilities",
    "tiingo://fundamentals/definitions",
    "tiingo://guide/date-formats",
]
PROMPT_CASES = {
    "analyze-stock": {"ticker": "AAPL"},
    "compare-stocks": {"ticker1": "AAPL", "ticker2": "MSFT"},
    "crypto-market-overview": {},
    "earnings-report-analysis": {"ticker": "NVDA", "earnings_date": "2024-02-21"},
    "forex-pair-analysis": {"pair": "eurusd"},
}


class ServerDiscoverRequest(
    mcp_types.Request[mcp_types.RequestParams | None, Literal["server/discover"]]
):
    method: Literal["server/discover"] = "server/discover"
    params: mcp_types.RequestParams | None = None


def canonical(value: Any) -> Any:
    if hasattr(value, "model_dump"):
        return canonical(value.model_dump(mode="json", by_alias=True, exclude_none=True))
    if isinstance(value, dict):
        return {str(key): canonical(item) for key, item in sorted(value.items())}
    if isinstance(value, (list, tuple)):
        return [canonical(item) for item in value]
    return value


async def replace_transport(
    client: TiingoClient, handler: Callable[[httpx.Request], httpx.Response]
) -> None:
    await client._client.aclose()
    client._client = httpx.AsyncClient(
        base_url=client.BASE_URL,
        headers={"Authorization": f"Token {client.api_key}"},
        transport=httpx.MockTransport(handler),
    )


async def capture_http_requests() -> list[dict[str, Any]]:
    seen: list[dict[str, Any]] = []

    def handler(request: httpx.Request) -> httpx.Response:
        seen.append(
            {
                "method": request.method,
                "path": request.url.path,
                "query": sorted(request.url.params.multi_items()),
                "authorization_scheme": request.headers["Authorization"].split()[0],
            }
        )
        return httpx.Response(200, json=[])

    client = TiingoClient(api_key="capture-key")
    await replace_transport(client, handler)
    await client.get_stock_metadata("AAPL")
    await client.get_stock_prices("AAPL", start_date="2024-01-01", end_date="2024-01-31", resample_freq="weekly")
    await client.get_realtime_price("AAPL", after_hours=True)
    await client.get_intraday_prices("AAPL", start_date="2024-01-01", end_date="2024-01-02", resample_freq="5min")
    await client.get_forex_quote("eurusd")
    await client.get_forex_prices("eurusd", start_date="2024-01-01", end_date="2024-01-02", resample_freq="1day")
    await client.get_crypto_quote("btcusd")
    await client.get_crypto_prices("btcusd", start_date="2024-01-01", end_date="2024-01-02", resample_freq="1hour")
    await client.get_crypto_metadata("btcusd")
    await client.get_news(tickers="AAPL", tags="earnings", source="reuters", start_date="2024-01-01", end_date="2024-01-31", limit=10, offset=5, sort_by="publishedDate")
    await client.get_fundamentals_definitions()
    await client.get_financial_statements("AAPL", start_date="2024-01-01", end_date="2024-01-31")
    await client.get_daily_fundamentals("AAPL", start_date="2024-01-01", end_date="2024-01-31")
    await client.get_company_meta("AAPL")
    await client.get_dividends("AAPL", start_date="2024-01-01", end_date="2024-01-31")
    await client.get_dividend_yield("AAPL", start_date="2024-01-01", end_date="2024-01-31")
    await client.get_splits("AAPL", start_date="2024-01-01", end_date="2024-01-31")
    await client.close()
    return seen


async def capture_error(
    handler: Callable[[httpx.Request], httpx.Response],
) -> dict[str, Any]:
    client = TiingoClient(api_key="capture-key")
    await replace_transport(client, handler)
    try:
        await client.get_stock_metadata("AAPL")
    except Exception as error:
        return {
            "exception": type(error).__name__,
            "status_code": getattr(error, "status_code", None),
            "detail": getattr(error, "detail", None),
            "message": str(error),
        }
    finally:
        await client.close()
    raise AssertionError("error case unexpectedly succeeded")


async def capture_errors() -> dict[str, Any]:
    cases: dict[str, Callable[[httpx.Request], httpx.Response]] = {
        str(status): (lambda request, status=status: httpx.Response(status, text=f"status {status}"))
        for status in (401, 403, 404, 429, 500)
    }
    cases["malformed_json"] = lambda request: httpx.Response(200, text="{")

    def timeout(request: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("captured timeout", request=request)

    cases["timeout"] = timeout
    return {name: await capture_error(handler) for name, handler in cases.items()}


async def capture() -> None:
    async with Client(mcp) as client:
        resources = FIXED_RESOURCES + [f"tiingo://guide/{name}" for name in GUIDES]
        try:
            await client.session.send_request(ServerDiscoverRequest(), mcp_types.Result)
        except McpError as error:
            server_discover = {"error": canonical(error.error)}
        else:
            raise AssertionError("Python 1.1.0 unexpectedly accepts server/discover")
        document = {
            "initialize": canonical(client.initialize_result),
            "server_discover": server_discover,
            "tools": canonical(await client.list_tools()),
            "resources": canonical(await client.list_resources()),
            "resource_templates": canonical(await client.list_resource_templates()),
            "resource_contents": {
                uri: canonical(await client.read_resource(uri)) for uri in resources
            },
            "prompts": canonical(await client.list_prompts()),
            "prompt_results": {
                name: canonical(await client.get_prompt(name, arguments=arguments))
                for name, arguments in PROMPT_CASES.items()
            },
        }
    document["http_requests"] = await capture_http_requests()
    document["errors"] = await capture_errors()
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    asyncio.run(capture())
```

- [ ] **Step 5: Generate and validate the contract fixture**

Run:

```bash
uv run python scripts/capture_python_contract.py
uv run pytest tests/test_contract_baseline.py tests/test_server.py tests/test_resources.py tests/test_prompts.py -v
```

Expected: the fixture is generated; all selected tests pass.

- [ ] **Step 6: Inspect the fixture for secrets and unstable values**

Run:

```bash
rg -n 'TIINGO_API_KEY|Token |test-api-key|capture-key|/Users/' tests/contract/baseline/python-mcp.json
git diff --check
```

Expected: `rg` prints no matches; `git diff --check` prints nothing.

- [ ] **Step 7: Commit the frozen baseline**

```bash
git add scripts/capture_python_contract.py tests/test_contract_baseline.py tests/contract/baseline/python-mcp.json
git commit -m "test: freeze Python MCP contract"
```

---

### Task 2: Add the minimal RMCP stdio binary foundation

**Files:**
- Create: `Cargo.toml`
- Create: `Cargo.lock`
- Create: `rust-toolchain.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`
- Create: `src/mcp/mod.rs`
- Create: `tests/cli.rs`

**Interfaces:**
- Consumes: no Rust application code.
- Produces: `tiingo_mcp::run_stdio() -> anyhow::Result<()>`, `mcp::TiingoServer::new()`, and the `tiingo-mcp` executable used by every later task.

- [ ] **Step 1: Write the failing CLI version test**

Create `tests/cli.rs`:

```rust
use assert_cmd::Command;

#[test]
fn version_flag_reports_release_version() {
    Command::cargo_bin("tiingo-mcp")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout("tiingo-mcp 2.0.0\n");
}
```

- [ ] **Step 2: Run the test and confirm Cargo metadata is absent**

Run: `cargo test --test cli`

Expected: FAIL because `Cargo.toml` does not exist.

- [ ] **Step 3: Add the root package and locked dependency policy**

Create `Cargo.toml` with this dependency boundary:

```toml
[package]
name = "tiingo-mcp"
version = "2.0.0"
edition = "2024"
rust-version = "1.88"
description = "MCP server for the Tiingo financial data API"
license = "MIT"
repository = "https://github.com/major7apps/tiingo-mcp"
keywords = ["mcp", "tiingo", "finance", "market-data"]
categories = ["command-line-utilities", "api-bindings"]
include = [
  "/src/**/*.rs",
  "/src/mcp/data/**/*.json",
  "/Cargo.toml",
  "/Cargo.lock",
  "/README.md",
  "/CHANGELOG.md",
  "/LICENSE",
]

[[bin]]
name = "tiingo-mcp"
path = "src/main.rs"

[dependencies]
anyhow = "1.0"
bytes = "1.10"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4.5", features = ["derive"] }
futures-util = "0.3"
httpdate = "1.0"
rand = "0.9"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "query", "rustls", "stream"] }
rmcp = { version = "=3.1.4", default-features = false, features = ["macros", "server", "transport-io"] }
schemars = { version = "1.0", features = ["chrono04"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
tokio = { version = "1.47", features = ["macros", "rt-multi-thread", "time"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
url = "2.5"

[dev-dependencies]
assert_cmd = "2.0"
predicates = "3.1"
rmcp = { version = "=3.1.4", default-features = false, features = ["client", "macros", "server", "transport-child-process", "transport-io"] }
sysinfo = "0.37"
wait-timeout = "0.2"
wiremock = "0.6"
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.88.0"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

- [ ] **Step 4: Add the smallest working RMCP server**

Create `src/mcp/mod.rs`:

```rust
use rmcp::{ServerHandler, model::{ServerCapabilities, ServerInfo}};

#[derive(Clone, Debug, Default)]
pub struct TiingoServer;

impl TiingoServer {
    pub fn new() -> Self {
        Self
    }
}

impl ServerHandler for TiingoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().build()).with_instructions(
            "Financial data server powered by Tiingo. Dates use YYYY-MM-DD.",
        )
    }
}
```

Create `src/lib.rs`:

```rust
pub mod mcp;

use rmcp::{ServiceExt, transport::stdio};

pub async fn run_stdio() -> anyhow::Result<()> {
    let service = mcp::TiingoServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

Create `src/main.rs`:

```rust
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "tiingo-mcp", version, about = "Tiingo financial-data MCP server")]
struct Cli {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
        .ok();
    tiingo_mcp::run_stdio().await
}
```

- [ ] **Step 5: Generate the lockfile and pass the foundation checks**

Run:

```bash
cargo generate-lockfile
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --test cli
```

Expected: every command passes and `Cargo.lock` pins RMCP 3.1.4.

- [ ] **Step 6: Verify default invocation is silent stdio and exits on EOF**

Run: `cargo run --quiet < /dev/null`

Expected: exit 0 with no stdout output.

- [ ] **Step 7: Commit the executable foundation**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml src/lib.rs src/main.rs src/mcp/mod.rs tests/cli.rs
git commit -m "build: add RMCP stdio foundation"
```

---

### Task 3: Implement the bounded authenticated HTTP core and error model

**Files:**
- Create: `src/config.rs`
- Create: `src/error.rs`
- Create: `src/client/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/client_http.rs`

**Interfaces:**
- Consumes: Tokio runtime and reqwest dependencies from Task 2.
- Produces: `Config`, `RetryPolicy`, `TiingoClient::new`, `TiingoClient::from_env`, `TiingoClient::get_json`, `TiingoError::payload`, and `MAX_RESPONSE_BYTES` for Tasks 4–6.

- [ ] **Step 1: Write failing tests for lazy credentials, redaction, retry classification, and the body cap**

Create `tests/client_http.rs` with these required assertions:

```rust
use std::time::Duration;
use tiingo_mcp::{client::TiingoClient, config::{Config, RetryPolicy}, error::TiingoError};
use url::Url;

fn test_config(base_url: Url, api_key: Option<&str>) -> Config {
    Config {
        api_key: api_key.map(str::to_owned),
        base_url,
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    }
}

#[tokio::test]
async fn missing_key_is_a_sanitized_configuration_error() {
    let client = TiingoClient::new(test_config(Url::parse("http://127.0.0.1:9").unwrap(), None)).unwrap();
    let error = client.get_json("stock metadata", "/tiingo/daily/AAPL", &[]).await.unwrap_err();
    assert!(matches!(error, TiingoError::Configuration(_)));
    assert!(!error.to_string().contains("Token "));
}

#[test]
fn only_safe_transient_statuses_retry() {
    assert!(TiingoError::status_is_retryable(429));
    assert!(TiingoError::status_is_retryable(502));
    assert!(TiingoError::status_is_retryable(503));
    assert!(TiingoError::status_is_retryable(504));
    for status in [400, 401, 403, 404, 500] {
        assert!(!TiingoError::status_is_retryable(status));
    }
}
```

Add Wiremock cases in the same file that assert: `Authorization: Token test-key` is sent; `None` query values are absent; a sequence `503, 503, 200` makes exactly three requests; a `401` makes one request; and an 8 MiB plus one byte response returns `TiingoError::ResponseTooLarge`.

- [ ] **Step 2: Run the tests and confirm the modules are missing**

Run: `cargo test --test client_http`

Expected: FAIL because `client`, `config`, and `error` do not exist.

- [ ] **Step 3: Add immutable runtime configuration**

Create `src/config.rs` with these exact public types and defaults:

```rust
use std::{env, time::Duration};
use url::Url;

pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter_max: Duration,
}

impl RetryPolicy {
    pub fn production() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(30),
            jitter_max: Duration::from_millis(250),
        }
    }

    pub fn test() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            jitter_max: Duration::ZERO,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: Option<String>,
    pub base_url: Url,
    pub request_timeout: Duration,
    pub retry: RetryPolicy,
    pub max_response_bytes: usize,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            api_key: env::var("TIINGO_API_KEY").ok().filter(|value| !value.is_empty()),
            base_url: Url::parse("https://api.tiingo.com")?,
            request_timeout: Duration::from_secs(30),
            retry: RetryPolicy::production(),
            max_response_bytes: MAX_RESPONSE_BYTES,
        })
    }
}
```

- [ ] **Step 4: Add the exhaustive caller-safe error enum**

Create `src/error.rs` with these variants:

```rust
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TiingoError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("invalid request: {0}")]
    Validation(String),
    #[error("Tiingo rejected the API credential while requesting {capability}")]
    Authentication { capability: &'static str },
    #[error("account entitlement required for {capability}")]
    Entitlement { capability: &'static str },
    #[error("resource not found for {capability}")]
    NotFound { capability: &'static str },
    #[error("Tiingo rate limit reached while requesting {capability}")]
    RateLimit { capability: &'static str },
    #[error("transient Tiingo failure ({status}) while requesting {capability}")]
    Transient { capability: &'static str, status: u16 },
    #[error("Tiingo request timed out while requesting {capability}")]
    Timeout { capability: &'static str },
    #[error("Tiingo transport failed while requesting {capability}")]
    Transport { capability: &'static str },
    #[error("Tiingo returned invalid JSON for {capability}")]
    Decode { capability: &'static str },
    #[error("Tiingo response for {capability} exceeded {limit} bytes")]
    ResponseTooLarge { capability: &'static str, limit: usize },
    #[error("Tiingo returned HTTP {status} for {capability}")]
    Upstream { capability: &'static str, status: u16, detail: String },
}

#[derive(Debug, Serialize)]
pub struct ErrorPayload<'a> {
    pub kind: &'a str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}
```

Implement `TiingoError::status_is_retryable`, `TiingoError::payload`, and status mapping so 401, 403, 404, and 429 become their dedicated variants. Every execution error names the requested capability and gives one caller action; the payload contains no raw response header and truncates upstream detail to 512 Unicode scalar values.

- [ ] **Step 5: Implement the shared bounded GET loop**

Create `src/client/mod.rs` with this public boundary:

```rust
use crate::{config::Config, error::TiingoError};
use bytes::BytesMut;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER}};
use serde_json::Value;
use std::time::{Duration, SystemTime};
use url::Url;

pub mod corporate_actions;
pub mod crypto;
pub mod eod;
pub mod forex;
pub mod fundamentals;
pub mod iex;
pub mod news;
pub mod query;

#[derive(Clone, Debug)]
pub struct TiingoClient {
    http: Client,
    config: Config,
}

impl TiingoClient {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let http = Client::builder().timeout(config.request_timeout).build()?;
        Ok(Self { http, config })
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::new(Config::from_env()?)
    }

    pub async fn get_json(
        &self,
        capability: &'static str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, TiingoError> {
        let api_key = self.config.api_key.as_deref().ok_or_else(|| {
            TiingoError::Configuration("set TIINGO_API_KEY before calling Tiingo tools".into())
        })?;
        let url = self.config.base_url.join(path).map_err(|_| {
            TiingoError::Validation("invalid Tiingo route".into())
        })?;
        let mut authorization = HeaderValue::from_str(&format!("Token {api_key}"))
            .map_err(|_| TiingoError::Configuration("TIINGO_API_KEY contains invalid characters".into()))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);

        self.send_with_retry(capability, url, headers, query).await
    }

    async fn send_with_retry(
        &self,
        capability: &'static str,
        url: Url,
        headers: HeaderMap,
        query: &[(&str, String)],
    ) -> Result<Value, TiingoError> {
        let mut last_error = None;
        for attempt in 1..=self.config.retry.max_attempts {
            match self.send_once(capability, url.clone(), headers.clone(), query).await {
                Ok(value) => return Ok(value),
                Err(failure) if failure.retryable && attempt < self.config.retry.max_attempts => {
                    let delay = failure.retry_after.unwrap_or_else(|| {
                        let exponent = u32::from(attempt.saturating_sub(1));
                        let base = self.config.retry.base_delay.saturating_mul(1_u32 << exponent);
                        let jitter_limit = self.config.retry.jitter_max.as_millis() as u64;
                        let jitter = Duration::from_millis(rand::random_range(0..=jitter_limit));
                        base.saturating_add(jitter).min(self.config.retry.max_delay)
                    });
                    last_error = Some(failure.error);
                    tokio::time::sleep(delay.min(self.config.retry.max_delay)).await;
                }
                Err(failure) => return Err(failure.error),
            }
        }
        Err(last_error.expect("max_attempts is non-zero"))
    }

    async fn send_once(
        &self,
        capability: &'static str,
        url: Url,
        headers: HeaderMap,
        query: &[(&str, String)],
    ) -> Result<Value, AttemptFailure> {
        let response = self.http.get(url).headers(headers).query(query).send().await
            .map_err(|error| AttemptFailure::transport(capability, error))?;
        let status = response.status();
        let retry_after = response.headers().get(RETRY_AFTER).and_then(parse_retry_after);

        if !status.is_success() {
            let detail = bounded_error_text(response, 512).await;
            tracing::warn!(capability, status = status.as_u16(), detail, "Tiingo request failed");
            return Err(AttemptFailure::status(capability, status, detail, retry_after));
        }

        let mut body = BytesMut::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| AttemptFailure::transport(capability, error))?;
            if body.len().saturating_add(chunk.len()) > self.config.max_response_bytes {
                return Err(AttemptFailure::terminal(TiingoError::ResponseTooLarge {
                    capability,
                    limit: self.config.max_response_bytes,
                }));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map_err(|_| AttemptFailure::terminal(TiingoError::Decode { capability }))
    }
}

struct AttemptFailure {
    error: TiingoError,
    retryable: bool,
    retry_after: Option<Duration>,
}

impl AttemptFailure {
    fn terminal(error: TiingoError) -> Self {
        Self { error, retryable: false, retry_after: None }
    }

    fn transport(capability: &'static str, error: reqwest::Error) -> Self {
        let tiingo_error = if error.is_timeout() {
            TiingoError::Timeout { capability }
        } else {
            TiingoError::Transport { capability }
        };
        Self { error: tiingo_error, retryable: error.is_timeout() || error.is_connect(), retry_after: None }
    }

    fn status(
        capability: &'static str,
        status: StatusCode,
        detail: String,
        retry_after: Option<Duration>,
    ) -> Self {
        let error = TiingoError::from_status(capability, status.as_u16(), detail);
        Self {
            retryable: TiingoError::status_is_retryable(status.as_u16()),
            error,
            retry_after,
        }
    }
}

fn parse_retry_after(value: &HeaderValue) -> Option<Duration> {
    let text = value.to_str().ok()?;
    if let Ok(seconds) = text.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(text).ok()?.duration_since(SystemTime::now()).ok()
}

async fn bounded_error_text(response: reqwest::Response, max_chars: usize) -> String {
    let max_bytes = max_chars.saturating_mul(4);
    let mut bytes = BytesMut::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = max_bytes.saturating_sub(bytes.len());
        if remaining == 0 { break; }
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    String::from_utf8_lossy(&bytes).chars().take(max_chars).collect()
}
```

`TiingoError::from_status` maps 401 to `Authentication`, 403 to `Entitlement`, 404 to `NotFound`, 429 to `RateLimit`, 502/503/504 to `Transient`, and every other non-success status to `Upstream`; each variant receives the `capability`. Add `RetryPolicy::validate()` and call it from `TiingoClient::new` so `max_attempts == 0` is rejected. Do not use reqwest middleware that can retry non-GET requests implicitly.

- [ ] **Step 6: Export the new modules and make all HTTP tests pass**

Modify `src/lib.rs`:

```rust
pub mod client;
pub mod config;
pub mod error;
pub mod mcp;
```

Run:

```bash
cargo fmt
cargo test --test client_http
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all client HTTP tests pass; Clippy is clean.

- [ ] **Step 7: Commit the HTTP core**

```bash
git add src/config.rs src/error.rs src/client/mod.rs src/lib.rs tests/client_http.rs
git commit -m "feat: add bounded Tiingo HTTP core"
```

---

### Task 4: Port EOD, IEX, and Forex routes with typed queries

**Files:**
- Create: `src/client/query.rs`
- Create: `src/client/eod.rs`
- Create: `src/client/iex.rs`
- Create: `src/client/forex.rs`
- Test: `tests/client_market_routes.rs`

**Interfaces:**
- Consumes: `TiingoClient::get_json` from Task 3.
- Produces: `DateRange`, `EodResample`, `IntradayResample`, and six client methods used by Task 6.

- [ ] **Step 1: Write failing route-table tests**

Create Wiremock tests in `tests/client_market_routes.rs` for this exact table:

| Method | Path | Query names |
|---|---|---|
| `get_stock_metadata("AAPL")` | `/tiingo/daily/AAPL` | none |
| `get_stock_prices("AAPL", populated_range, Some(EodResample::Weekly))` | `/tiingo/daily/AAPL/prices` | `startDate`, `endDate`, `resampleFreq` |
| `get_realtime_price("AAPL", Some(true))` | `/iex/AAPL` | `afterHours=true` |
| `get_intraday_prices("AAPL", populated_range, Some(IntradayResample::FiveMinutes))` | `/iex/AAPL/prices` | `startDate`, `endDate`, `resampleFreq` |
| `get_forex_quote("eurusd")` | `/tiingo/fx/eurusd/top` | none |
| `get_forex_prices("eurusd", populated_range, Some(IntradayResample::OneDay))` | `/tiingo/fx/eurusd/prices` | `startDate`, `endDate`, `resampleFreq` |

Also assert that a path symbol containing `/`, `?`, `#`, or `%` returns `TiingoError::Validation` before Wiremock receives a request.

- [ ] **Step 2: Run the route tests and confirm the methods are absent**

Run: `cargo test --test client_market_routes`

Expected: compile failure for the six missing methods.

- [ ] **Step 3: Add shared typed query values**

Create `src/client/query.rs` with `serde::Serialize`, `serde::Deserialize`, and `schemars::JsonSchema` types:

```rust
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EodResample { Daily, Weekly, Monthly, Annually }

impl EodResample {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Annually => "annually",
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub enum IntradayResample {
    #[serde(rename = "1min")] OneMinute,
    #[serde(rename = "5min")] FiveMinutes,
    #[serde(rename = "15min")] FifteenMinutes,
    #[serde(rename = "30min")] ThirtyMinutes,
    #[serde(rename = "1hour")] OneHour,
    #[serde(rename = "1day")] OneDay,
}

impl IntradayResample {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1min",
            Self::FiveMinutes => "5min",
            Self::FifteenMinutes => "15min",
            Self::ThirtyMinutes => "30min",
            Self::OneHour => "1hour",
            Self::OneDay => "1day",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DateRange {
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
}

impl DateRange {
    pub fn append(self, query: &mut Vec<(&'static str, String)>) {
        if let Some(value) = self.start_date {
            query.push(("startDate", value.format("%Y-%m-%d").to_string()));
        }
        if let Some(value) = self.end_date {
            query.push(("endDate", value.format("%Y-%m-%d").to_string()));
        }
    }
}
```

Add `validate_path_segment(&str) -> Result<(), TiingoError>` allowing only non-empty ASCII alphanumeric characters plus `.`, `_`, `-`, and `:`.

- [ ] **Step 4: Implement the six exact route methods**

Create `src/client/eod.rs`:

```rust
use super::{TiingoClient, query::{DateRange, EodResample, validate_path_segment}};
use crate::error::TiingoError;

impl TiingoClient {
pub async fn get_stock_metadata(&self, ticker: &str) -> Result<serde_json::Value, TiingoError> {
    validate_path_segment(ticker)?;
    self.get_json("stock metadata", &format!("/tiingo/daily/{ticker}"), &[]).await
}

pub async fn get_stock_prices(
    &self,
    ticker: &str,
    range: DateRange,
    resample: Option<EodResample>,
) -> Result<serde_json::Value, TiingoError> {
    validate_path_segment(ticker)?;
    let mut query = Vec::new();
    range.append(&mut query);
    if let Some(value) = resample {
        query.push(("resampleFreq", value.as_str().to_owned()));
    }
    self.get_json("stock prices", &format!("/tiingo/daily/{ticker}/prices"), &query).await
}
}
```

Create `src/client/iex.rs`:

```rust
use super::{TiingoClient, query::{DateRange, IntradayResample, validate_path_segment}};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_realtime_price(
        &self,
        ticker: &str,
        after_hours: Option<bool>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let query = after_hours
            .map(|value| vec![("afterHours", value.to_string())])
            .unwrap_or_default();
        self.get_json("real-time stock price", &format!("/iex/{ticker}"), &query).await
    }

    pub async fn get_intraday_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json("intraday stock prices", &format!("/iex/{ticker}/prices"), &query).await
    }
}
```

Create `src/client/forex.rs`:

```rust
use super::{TiingoClient, query::{DateRange, IntradayResample, validate_path_segment}};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_forex_quote(&self, ticker: &str) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json("forex quote", &format!("/tiingo/fx/{ticker}/top"), &[]).await
    }

    pub async fn get_forex_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json("forex prices", &format!("/tiingo/fx/{ticker}/prices"), &query).await
    }
}
```

- [ ] **Step 5: Pass all market route and shared HTTP tests**

Run:

```bash
cargo test --test client_market_routes --test client_http
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all tests pass.

- [ ] **Step 6: Commit market route parity**

```bash
git add src/client/query.rs src/client/eod.rs src/client/iex.rs src/client/forex.rs tests/client_market_routes.rs
git commit -m "feat: port stock IEX and forex routes"
```

---

### Task 5: Port Crypto, News, Fundamentals, and Corporate Actions routes

**Files:**
- Create: `src/client/crypto.rs`
- Create: `src/client/news.rs`
- Create: `src/client/fundamentals.rs`
- Create: `src/client/corporate_actions.rs`
- Test: `tests/client_data_routes.rs`

**Interfaces:**
- Consumes: `TiingoClient::get_json`, `DateRange`, `IntradayResample`, and path validation.
- Produces: the remaining 11 client methods used by Task 6.

- [ ] **Step 1: Write failing exact route tests**

Create `tests/client_data_routes.rs` for this complete mapping:

| Method | Path | Query names |
|---|---|---|
| `get_crypto_quote` | `/tiingo/crypto/prices` | `tickers` |
| `get_crypto_prices` | `/tiingo/crypto/prices` | `tickers`, `startDate`, `endDate`, `resampleFreq` |
| `get_crypto_metadata` | `/tiingo/crypto` | `tickers` |
| `get_news` | `/tiingo/news` | `tickers`, `tags`, `source`, `startDate`, `endDate`, `limit`, `offset`, `sortBy` |
| `get_fundamentals_definitions` | `/tiingo/fundamentals/definitions` | none |
| `get_financial_statements` | `/tiingo/fundamentals/{ticker}/statements` | `startDate`, `endDate` |
| `get_daily_fundamentals` | `/tiingo/fundamentals/{ticker}/daily` | `startDate`, `endDate` |
| `get_company_meta` | `/tiingo/fundamentals/meta` | `tickers` |
| `get_dividends` | `/tiingo/corporate-actions/{ticker}/distributions` | `startExDate`, `endExDate` |
| `get_dividend_yield` | `/tiingo/corporate-actions/{ticker}/distribution-yield` | `startDate`, `endDate` |
| `get_splits` | `/tiingo/corporate-actions/{ticker}/splits` | `startExDate`, `endExDate` |

The crypto quote test must fail if the implementation calls `/tiingo/crypto/top`. News route tests use `u32` limits/offsets and assert their exact decimal serialization; negative wire values are rejected at the MCP schema boundary in Task 6.

- [ ] **Step 2: Run and confirm the remaining methods are absent**

Run: `cargo test --test client_data_routes`

Expected: compile failure for the missing methods.

- [ ] **Step 3: Add the remaining closed query enum and request value types**

Add to `src/client/query.rs`:

```rust
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub enum NewsSort {
    #[serde(rename = "crawlDate")] CrawlDate,
    #[serde(rename = "publishedDate")] PublishedDate,
}

impl NewsSort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CrawlDate => "crawlDate",
            Self::PublishedDate => "publishedDate",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NewsQuery {
    pub tickers: Option<String>,
    pub tags: Option<String>,
    pub source: Option<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub sort_by: Option<NewsSort>,
}
```

- [ ] **Step 4: Implement all 11 methods from the route table**

Create `src/client/crypto.rs`:

```rust
use super::{TiingoClient, query::{DateRange, IntradayResample}};
use crate::error::TiingoError;

fn tickers_query(tickers: Option<&str>) -> Vec<(&'static str, String)> {
    tickers.filter(|value| !value.is_empty())
        .map(|value| vec![("tickers", value.to_owned())])
        .unwrap_or_default()
}

impl TiingoClient {
    pub async fn get_crypto_quote(
        &self,
        tickers: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_json("current crypto prices", "/tiingo/crypto/prices", &tickers_query(tickers)).await
    }

    pub async fn get_crypto_prices(
        &self,
        tickers: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        if tickers.is_empty() {
            return Err(TiingoError::Validation("tickers cannot be empty".into()));
        }
        let mut query = vec![("tickers", tickers.to_owned())];
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json("crypto prices", "/tiingo/crypto/prices", &query).await
    }

    pub async fn get_crypto_metadata(
        &self,
        tickers: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_json("crypto metadata", "/tiingo/crypto", &tickers_query(tickers)).await
    }
}
```

Create `src/client/news.rs`:

```rust
use super::{TiingoClient, query::{DateRange, NewsQuery}};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_news(&self, values: NewsQuery) -> Result<serde_json::Value, TiingoError> {
        let mut query = Vec::new();
        for (name, value) in [
            ("tickers", values.tickers),
            ("tags", values.tags),
            ("source", values.source),
        ] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                query.push((name, value));
            }
        }
        DateRange { start_date: values.start_date, end_date: values.end_date }.append(&mut query);
        if let Some(value) = values.limit { query.push(("limit", value.to_string())); }
        if let Some(value) = values.offset { query.push(("offset", value.to_string())); }
        if let Some(value) = values.sort_by { query.push(("sortBy", value.as_str().to_owned())); }
        self.get_json("news", "/tiingo/news", &query).await
    }
}
```

`u32` makes negative `limit` and `offset` fail schema deserialization before this method is called.

Create `src/client/fundamentals.rs`:

```rust
use super::{TiingoClient, query::{DateRange, validate_path_segment}};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_fundamentals_definitions(&self) -> Result<serde_json::Value, TiingoError> {
        self.get_json("fundamentals definitions", "/tiingo/fundamentals/definitions", &[]).await
    }

    pub async fn get_financial_statements(&self, ticker: &str, range: DateRange) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        self.get_json("financial statements", &format!("/tiingo/fundamentals/{ticker}/statements"), &query).await
    }

    pub async fn get_daily_fundamentals(&self, ticker: &str, range: DateRange) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        self.get_json("daily fundamentals", &format!("/tiingo/fundamentals/{ticker}/daily"), &query).await
    }

    pub async fn get_company_meta(&self, tickers: &str) -> Result<serde_json::Value, TiingoError> {
        if tickers.is_empty() {
            return Err(TiingoError::Validation("tickers cannot be empty".into()));
        }
        self.get_json("company metadata", "/tiingo/fundamentals/meta", &[("tickers", tickers.to_owned())]).await
    }
}
```

Create `src/client/corporate_actions.rs`:

```rust
use super::{TiingoClient, query::{DateRange, validate_path_segment}};
use crate::error::TiingoError;

fn ex_date_query(range: DateRange) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(value) = range.start_date {
        query.push(("startExDate", value.format("%Y-%m-%d").to_string()));
    }
    if let Some(value) = range.end_date {
        query.push(("endExDate", value.format("%Y-%m-%d").to_string()));
    }
    query
}

impl TiingoClient {
    pub async fn get_dividends(&self, ticker: &str, range: DateRange) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json("dividends", &format!("/tiingo/corporate-actions/{ticker}/distributions"), &ex_date_query(range)).await
    }

    pub async fn get_dividend_yield(&self, ticker: &str, range: DateRange) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        self.get_json("dividend yield", &format!("/tiingo/corporate-actions/{ticker}/distribution-yield"), &query).await
    }

    pub async fn get_splits(&self, ticker: &str, range: DateRange) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json("splits", &format!("/tiingo/corporate-actions/{ticker}/splits"), &ex_date_query(range)).await
    }
}
```

- [ ] **Step 5: Pass every client test**

Run:

```bash
cargo test --test client_http --test client_market_routes --test client_data_routes
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all tests pass and Clippy is clean.

- [ ] **Step 6: Commit complete REST parity**

```bash
git add src/client/query.rs src/client/crypto.rs src/client/news.rs src/client/fundamentals.rs src/client/corporate_actions.rs tests/client_data_routes.rs
git commit -m "feat: port Tiingo data and corporate routes"
```

---

### Task 6: Expose all 17 typed RMCP tools and structured error results

**Files:**
- Create: `src/mcp/tools.rs`
- Modify: `src/mcp/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/mcp_tools.rs`

**Interfaces:**
- Consumes: all 17 `TiingoClient` methods.
- Produces: `TiingoServer::with_client(TiingoClient)`, RMCP tool routing, legacy text plus structured success results, and structured `isError: true` results.

- [ ] **Step 1: Write failing tool discovery and result-shape tests**

Create `tests/mcp_tools.rs` that connects in-process to `TiingoServer::with_client(test_client)` and asserts:

```rust
const TOOL_NAMES: [&str; 17] = [
    "get_stock_metadata", "get_stock_prices", "get_realtime_price",
    "get_intraday_prices", "get_forex_quote", "get_forex_prices",
    "get_crypto_quote", "get_crypto_prices", "get_crypto_metadata", "get_news",
    "get_fundamentals_definitions", "get_financial_statements",
    "get_daily_fundamentals", "get_company_meta", "get_dividends",
    "get_dividend_yield", "get_splits",
];
```

For a Wiremock `[{"ticker":"AAPL"}]` response, assert content block zero parses to the original array and `structured_content == {"data":[{"ticker":"AAPL"}],"meta":{"source":"tiingo"}}`. For a 401 response, assert `is_error == Some(true)`, structured error kind is `authentication`, and neither content nor structured data contains the test key.

Call `get_news` once with `{"limit":-1}` and once with `{"offset":-1}`; both calls must return invalid-params errors and Wiremock must record zero requests.

- [ ] **Step 2: Run and confirm the tool router is absent**

Run: `cargo test --test mcp_tools`

Expected: compile failure because `with_client` and tool routes do not exist.

- [ ] **Step 3: Add typed argument structs with exact public field names**

Create `src/mcp/tools.rs`. Derive `serde::Deserialize` and `schemars::JsonSchema` for dedicated structs covering this matrix:

| Tool | Required fields | Optional fields |
|---|---|---|
| `get_stock_metadata` | `ticker` | none |
| `get_stock_prices` | `ticker` | `start_date`, `end_date`, `resample_freq: EodResample` |
| `get_realtime_price` | `ticker` | `after_hours` |
| `get_intraday_prices` | `ticker` | `start_date`, `end_date`, `resample_freq: IntradayResample` |
| `get_forex_quote` | `ticker` | none |
| `get_forex_prices` | `ticker` | `start_date`, `end_date`, `resample_freq: IntradayResample` |
| `get_crypto_quote` | none | `tickers` |
| `get_crypto_prices` | `tickers` | `start_date`, `end_date`, `resample_freq: IntradayResample` |
| `get_crypto_metadata` | none | `tickers` |
| `get_news` | none | `tickers`, `tags`, `source`, `start_date`, `end_date`, `limit`, `offset`, `sort_by: NewsSort` |
| `get_fundamentals_definitions` | none | none |
| `get_financial_statements` | `ticker` | `start_date`, `end_date` |
| `get_daily_fundamentals` | `ticker` | `start_date`, `end_date` |
| `get_company_meta` | `tickers` | none |
| `get_dividends` | `ticker` | `start_date`, `end_date` |
| `get_dividend_yield` | `ticker` | `start_date`, `end_date` |
| `get_splits` | `ticker` | `start_date`, `end_date` |

Dates are `Option<chrono::NaiveDate>`. `limit` and `offset` are `Option<u32>`. Define the public schema types exactly as follows:

```rust
use crate::client::query::{EodResample, IntradayResample, NewsSort};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StockMetadataArgs { pub ticker: String }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StockPricesArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate>, pub resample_freq: Option<EodResample> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RealtimePriceArgs { pub ticker: String, pub after_hours: Option<bool> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IntradayPricesArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate>, pub resample_freq: Option<IntradayResample> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForexQuoteArgs { pub ticker: String }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForexPricesArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate>, pub resample_freq: Option<IntradayResample> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CryptoQuoteArgs { pub tickers: Option<String> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CryptoPricesArgs { pub tickers: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate>, pub resample_freq: Option<IntradayResample> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CryptoMetadataArgs { pub tickers: Option<String> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NewsArgs { pub tickers: Option<String>, pub tags: Option<String>, pub source: Option<String>, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate>, pub limit: Option<u32>, pub offset: Option<u32>, pub sort_by: Option<NewsSort> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FinancialStatementsArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DailyFundamentalsArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CompanyMetaArgs { pub tickers: String }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DividendsArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DividendYieldArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate> }
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SplitsArgs { pub ticker: String, pub start_date: Option<chrono::NaiveDate>, pub end_date: Option<chrono::NaiveDate> }
```

Run `cargo fmt`, then add Rust `///` field comments copied verbatim from the matching Python `Args:` entry. These comments become the schema descriptions; the public field names and types above are normative.

- [ ] **Step 4: Add deterministic success and failure conversion**

Implement these helpers in `src/mcp/tools.rs`:

```rust
fn success_result(value: serde_json::Value) -> rmcp::model::CallToolResult {
    let text = serde_json::to_string_pretty(&value).expect("JSON value serializes");
    let mut result = rmcp::model::CallToolResult::success(vec![
        rmcp::model::ContentBlock::text(text),
    ]);
    result.structured_content = Some(serde_json::json!({
        "data": value,
        "meta": { "source": "tiingo" }
    }));
    result
}

fn error_result(error: crate::error::TiingoError) -> rmcp::model::CallToolResult {
    let payload = error.payload();
    let text = serde_json::to_string_pretty(&payload).expect("error payload serializes");
    let mut result = rmcp::model::CallToolResult::error(vec![
        rmcp::model::ContentBlock::text(text),
    ]);
    result.structured_content = Some(serde_json::json!({ "error": payload }));
    result
}
```

- [ ] **Step 5: Add all 17 RMCP routes in one tool router**

Add these helpers and the complete router to `src/mcp/tools.rs`:

```rust
use crate::client::query::{DateRange, NewsQuery};
use rmcp::{handler::server::tool::Parameters, model::CallToolResult};

fn tool_result(response: Result<serde_json::Value, crate::error::TiingoError>) -> CallToolResult {
    match response {
        Ok(value) => success_result(value),
        Err(error) => error_result(error),
    }
}

fn range(start_date: Option<chrono::NaiveDate>, end_date: Option<chrono::NaiveDate>) -> DateRange {
    DateRange { start_date, end_date }
}

#[rmcp::tool_router]
impl TiingoServer {
    #[rmcp::tool(description = "Get metadata for a stock ticker including name, exchange, description, and date range.")]
    async fn get_stock_metadata(&self, Parameters(args): Parameters<StockMetadataArgs>) -> CallToolResult {
        tool_result(self.client.get_stock_metadata(&args.ticker).await)
    }

    #[rmcp::tool(description = "Get historical end-of-day stock prices with adjusted and unadjusted OHLCV data.")]
    async fn get_stock_prices(&self, Parameters(args): Parameters<StockPricesArgs>) -> CallToolResult {
        tool_result(self.client.get_stock_prices(&args.ticker, range(args.start_date, args.end_date), args.resample_freq).await)
    }

    #[rmcp::tool(description = "Get the current real-time IEX top-of-book price for a stock.")]
    async fn get_realtime_price(&self, Parameters(args): Parameters<RealtimePriceArgs>) -> CallToolResult {
        tool_result(self.client.get_realtime_price(&args.ticker, args.after_hours).await)
    }

    #[rmcp::tool(description = "Get historical intraday prices from IEX at supported intervals.")]
    async fn get_intraday_prices(&self, Parameters(args): Parameters<IntradayPricesArgs>) -> CallToolResult {
        tool_result(self.client.get_intraday_prices(&args.ticker, range(args.start_date, args.end_date), args.resample_freq).await)
    }

    #[rmcp::tool(description = "Get the current top-of-book forex quote for a currency pair.")]
    async fn get_forex_quote(&self, Parameters(args): Parameters<ForexQuoteArgs>) -> CallToolResult {
        tool_result(self.client.get_forex_quote(&args.ticker).await)
    }

    #[rmcp::tool(description = "Get historical forex prices for a currency pair.")]
    async fn get_forex_prices(&self, Parameters(args): Parameters<ForexPricesArgs>) -> CallToolResult {
        tool_result(self.client.get_forex_prices(&args.ticker, range(args.start_date, args.end_date), args.resample_freq).await)
    }

    #[rmcp::tool(description = "Get current crypto prices, optionally filtered by ticker.")]
    async fn get_crypto_quote(&self, Parameters(args): Parameters<CryptoQuoteArgs>) -> CallToolResult {
        tool_result(self.client.get_crypto_quote(args.tickers.as_deref()).await)
    }

    #[rmcp::tool(description = "Get historical crypto prices.")]
    async fn get_crypto_prices(&self, Parameters(args): Parameters<CryptoPricesArgs>) -> CallToolResult {
        tool_result(self.client.get_crypto_prices(&args.tickers, range(args.start_date, args.end_date), args.resample_freq).await)
    }

    #[rmcp::tool(description = "Get metadata for crypto tickers including supported exchanges and pairs.")]
    async fn get_crypto_metadata(&self, Parameters(args): Parameters<CryptoMetadataArgs>) -> CallToolResult {
        tool_result(self.client.get_crypto_metadata(args.tickers.as_deref()).await)
    }

    #[rmcp::tool(description = "Search financial news articles by ticker, tag, source, date, and sort order.")]
    async fn get_news(&self, Parameters(args): Parameters<NewsArgs>) -> CallToolResult {
        tool_result(self.client.get_news(NewsQuery {
            tickers: args.tickers,
            tags: args.tags,
            source: args.source,
            start_date: args.start_date,
            end_date: args.end_date,
            limit: args.limit,
            offset: args.offset,
            sort_by: args.sort_by,
        }).await)
    }

    #[rmcp::tool(description = "Get definitions for Tiingo fundamental data fields.")]
    async fn get_fundamentals_definitions(&self) -> CallToolResult {
        tool_result(self.client.get_fundamentals_definitions().await)
    }

    #[rmcp::tool(description = "Get quarterly and annual financial statements for a company.")]
    async fn get_financial_statements(&self, Parameters(args): Parameters<FinancialStatementsArgs>) -> CallToolResult {
        tool_result(self.client.get_financial_statements(&args.ticker, range(args.start_date, args.end_date)).await)
    }

    #[rmcp::tool(description = "Get daily fundamental metrics such as market cap and valuation ratios.")]
    async fn get_daily_fundamentals(&self, Parameters(args): Parameters<DailyFundamentalsArgs>) -> CallToolResult {
        tool_result(self.client.get_daily_fundamentals(&args.ticker, range(args.start_date, args.end_date)).await)
    }

    #[rmcp::tool(description = "Get company metadata including sector, industry, country, and SIC code.")]
    async fn get_company_meta(&self, Parameters(args): Parameters<CompanyMetaArgs>) -> CallToolResult {
        tool_result(self.client.get_company_meta(&args.tickers).await)
    }

    #[rmcp::tool(description = "Get dividend distribution history for a ticker.")]
    async fn get_dividends(&self, Parameters(args): Parameters<DividendsArgs>) -> CallToolResult {
        tool_result(self.client.get_dividends(&args.ticker, range(args.start_date, args.end_date)).await)
    }

    #[rmcp::tool(description = "Get historical dividend yield for a ticker.")]
    async fn get_dividend_yield(&self, Parameters(args): Parameters<DividendYieldArgs>) -> CallToolResult {
        tool_result(self.client.get_dividend_yield(&args.ticker, range(args.start_date, args.end_date)).await)
    }

    #[rmcp::tool(description = "Get stock split history for a ticker.")]
    async fn get_splits(&self, Parameters(args): Parameters<SplitsArgs>) -> CallToolResult {
        tool_result(self.client.get_splits(&args.ticker, range(args.start_date, args.end_date)).await)
    }
}
```

Do not duplicate HTTP path construction in the MCP layer or add phase-two parameters.

- [ ] **Step 6: Make `TiingoServer` own the client and tool router**

Change `src/mcp/mod.rs` to this structural boundary:

```rust
use std::sync::Arc;
use rmcp::handler::server::router::tool::ToolRouter;
use crate::client::TiingoClient;

#[derive(Clone, Debug)]
pub struct TiingoServer {
    pub(crate) client: Arc<TiingoClient>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl TiingoServer {
    pub fn with_client(client: TiingoClient) -> Self {
        Self { client: Arc::new(client), tool_router: Self::tool_router() }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self::with_client(TiingoClient::from_env()?))
    }
}
```

Change `run_stdio` to call `TiingoServer::from_env()`. Because `Config::from_env` permits a missing key, discovery still starts without credentials.

- [ ] **Step 7: Pass tool discovery, schema, success, and error tests**

Run:

```bash
cargo test --test mcp_tools
cargo test --test client_http --test client_market_routes --test client_data_routes
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: 17 tools are listed; structured success and error assertions pass.

- [ ] **Step 8: Commit the complete RMCP tool surface**

```bash
git add src/mcp/tools.rs src/mcp/mod.rs src/lib.rs tests/mcp_tools.rs
git commit -m "feat: expose Tiingo RMCP tools"
```

---

### Task 7: Port and correct resources without changing their identifiers

**Files:**
- Create: `scripts/export_rust_resources.py`
- Create: `src/mcp/resources.rs`
- Create: `src/mcp/data/capabilities.json`
- Create: `src/mcp/data/fundamentals-definitions.json`
- Create: `src/mcp/data/date-formats.json`
- Create: `src/mcp/data/guides/{corporate-actions,crypto,forex,fundamentals,news,stocks}.json`
- Modify: `src/mcp/mod.rs`
- Test: `tests/mcp_resources.rs`

**Interfaces:**
- Consumes: frozen Python resource fixture and `TiingoServer`.
- Produces: RMCP `resources/list`, `resources/templates/list`, and `resources/read` handlers for Task 9.

- [ ] **Step 1: Write failing resource contract tests**

Create `tests/mcp_resources.rs` and assert:

- exactly three fixed resources with URIs `tiingo://capabilities`, `tiingo://fundamentals/definitions`, and `tiingo://guide/date-formats`;
- exactly one template `tiingo://guide/{asset_class}`;
- all six guide values return JSON;
- an invalid guide returns the existing JSON error shape;
- capabilities contains `server_version: "2.0.0"`, `tool_count: 17`, `as_of: "2026-08-24"`, and source URLs;
- no resource contains `5000 req/hr`, `50,000`, `All fundamentals endpoints available on free tier`, or `BRK.B`;
- the stocks guide contains `BRK-A`.

- [ ] **Step 2: Run and confirm resource handlers are missing**

Run: `cargo test --test mcp_resources`

Expected: FAIL because the server does not advertise resources.

- [ ] **Step 3: Convert the frozen content to compile-time JSON data**

Create `scripts/export_rust_resources.py` to make the conversion deterministic:

```python
from __future__ import annotations

import asyncio
import json
from pathlib import Path

from fastmcp import Client
from tiingo_mcp.server import mcp

OUT = Path("src/mcp/data")
AS_OF = "2026-08-24"
SOURCES = [
    "https://www.tiingo.com/documentation/general/overview",
    "https://api.tiingo.com/documentation/end-of-day",
]
GUIDES = ["corporate-actions", "crypto", "forex", "fundamentals", "news", "stocks"]


async def read_json(client: Client, uri: str) -> dict:
    contents = await client.read_resource(uri)
    return json.loads(contents[0].text)


def write(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


async def main() -> None:
    async with Client(mcp) as client:
        capabilities = await read_json(client, "tiingo://capabilities")
        capabilities["server_version"] = "2.0.0"
        capabilities["as_of"] = AS_OF
        capabilities["entitlements_change_over_time"] = True
        capabilities["official_sources"] = SOURCES
        capabilities.pop("rate_limits", None)
        capabilities.pop("plan_restrictions", None)
        write(OUT / "capabilities.json", capabilities)

        write(
            OUT / "fundamentals-definitions.json",
            await read_json(client, "tiingo://fundamentals/definitions"),
        )
        write(OUT / "date-formats.json", await read_json(client, "tiingo://guide/date-formats"))

        for name in GUIDES:
            guide = await read_json(client, f"tiingo://guide/{name}")
            guide.pop("plan_restrictions", None)
            guide["availability"] = {
                "as_of": AS_OF,
                "statement": "Access depends on current Tiingo account entitlements; a 403 means this credential is not entitled to the requested capability.",
                "official_sources": SOURCES,
            }
            guide["common_pitfalls"] = [
                item for item in guide["common_pitfalls"]
                if not any(word in item.lower() for word in ("free tier", "paid plan"))
            ]
            if name == "stocks":
                guide = json.loads(json.dumps(guide).replace("BRK.B", "BRK-A"))
            if name == "crypto":
                guide["current_price_route"] = "/tiingo/crypto/prices"
            write(OUT / "guides" / f"{name}.json", guide)


if __name__ == "__main__":
    asyncio.run(main())
```

This preserves the stable metric lists, date/resample lists, tool lists, and guide workflows while making these exact corrections:

- replace changing plan/rate assertions with `as_of: "2026-08-24"`, `entitlements_change_over_time: true`, and official Tiingo documentation URLs;
- describe 403 as account entitlement required rather than naming Power or Business;
- use `BRK-A` and dash symbology;
- describe `get_crypto_quote` as current prices from `/tiingo/crypto/prices`;
- keep all existing asset-class keys, tool lists, and workflow coverage.

Run the converter and validate every data file:

```bash
uv run python scripts/export_rust_resources.py
jq empty src/mcp/data/*.json src/mcp/data/guides/*.json
```

Expected: exit 0.

- [ ] **Step 4: Implement explicit resource dispatch**

Create `src/mcp/resources.rs` with this complete dispatch table:

```rust
use rmcp::{ErrorData, model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate}};

const CAPABILITIES: &str = include_str!("data/capabilities.json");
const DEFINITIONS: &str = include_str!("data/fundamentals-definitions.json");
const DATE_FORMATS: &str = include_str!("data/date-formats.json");
const GUIDE_CORPORATE_ACTIONS: &str = include_str!("data/guides/corporate-actions.json");
const GUIDE_CRYPTO: &str = include_str!("data/guides/crypto.json");
const GUIDE_FOREX: &str = include_str!("data/guides/forex.json");
const GUIDE_FUNDAMENTALS: &str = include_str!("data/guides/fundamentals.json");
const GUIDE_NEWS: &str = include_str!("data/guides/news.json");
const GUIDE_STOCKS: &str = include_str!("data/guides/stocks.json");

pub fn list() -> Vec<Resource> {
    vec![
        Resource::new("tiingo://capabilities", "capabilities")
            .with_description("Server capabilities and source-dated entitlement guidance")
            .with_mime_type("application/json"),
        Resource::new("tiingo://fundamentals/definitions", "fundamentals-definitions")
            .with_description("Curated common fundamental metric definitions")
            .with_mime_type("application/json"),
        Resource::new("tiingo://guide/date-formats", "date-formats")
            .with_description("Date, resampling, sorting, and corporate-action parameter reference")
            .with_mime_type("application/json"),
    ]
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![ResourceTemplate::new("tiingo://guide/{asset_class}", "asset-class-guide")
        .with_description("Tools, workflows, symbology, and pitfalls for one asset class")
        .with_mime_type("application/json")]
}

pub fn read(uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let json = match uri {
        "tiingo://capabilities" => CAPABILITIES,
        "tiingo://fundamentals/definitions" => DEFINITIONS,
        "tiingo://guide/date-formats" => DATE_FORMATS,
        "tiingo://guide/corporate-actions" => GUIDE_CORPORATE_ACTIONS,
        "tiingo://guide/crypto" => GUIDE_CRYPTO,
        "tiingo://guide/forex" => GUIDE_FOREX,
        "tiingo://guide/fundamentals" => GUIDE_FUNDAMENTALS,
        "tiingo://guide/news" => GUIDE_NEWS,
        "tiingo://guide/stocks" => GUIDE_STOCKS,
        value if value.starts_with("tiingo://guide/") => {
            let name = &value["tiingo://guide/".len()..];
            let error = serde_json::json!({
                "error": format!(
                    "Invalid asset class '{name}'. Valid values: corporate-actions, crypto, forex, fundamentals, news, stocks"
                )
            });
            return Ok(ReadResourceResult::new(vec![
                ResourceContents::text(error.to_string(), uri).with_mime_type("application/json"),
            ]));
        }
        _ => return Err(ErrorData::resource_not_found("resource not found", Some(serde_json::json!({ "uri": uri })))),
    };
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(json, uri).with_mime_type("application/json"),
    ]))
}
```

- [ ] **Step 5: Wire resources into the single ServerHandler implementation**

Annotate the existing implementation with `#[rmcp::tool_handler]` and advertise:

```rust
ServerCapabilities::builder()
    .enable_tools()
    .enable_resources()
    .build()
```

Add these methods to the existing handler; underscore-prefixed arguments are intentionally unused:

```rust
async fn list_resources(
    &self,
    _request: Option<rmcp::model::PaginatedRequestParams>,
    _context: rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<rmcp::model::ListResourcesResult, rmcp::ErrorData> {
    Ok(rmcp::model::ListResourcesResult::with_all_items(resources::list()))
}

async fn list_resource_templates(
    &self,
    _request: Option<rmcp::model::PaginatedRequestParams>,
    _context: rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<rmcp::model::ListResourceTemplatesResult, rmcp::ErrorData> {
    Ok(rmcp::model::ListResourceTemplatesResult::with_all_items(resources::templates()))
}

async fn read_resource(
    &self,
    request: rmcp::model::ReadResourceRequestParams,
    _context: rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<rmcp::model::ReadResourceResponse, rmcp::ErrorData> {
    Ok(resources::read(&request.uri)?.into())
}
```

- [ ] **Step 6: Pass resource and tool regressions**

Run:

```bash
cargo test --test mcp_resources --test mcp_tools
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all tests pass; tool routing still works through `#[tool_handler]`.

- [ ] **Step 7: Commit resource parity and corrections**

```bash
git add scripts/export_rust_resources.py src/mcp/resources.rs src/mcp/data src/mcp/mod.rs tests/mcp_resources.rs
git commit -m "feat: port corrected MCP resources"
```

---

### Task 8: Port the five prompts and remove unsupported conclusions

**Files:**
- Create: `src/mcp/prompts.rs`
- Modify: `src/mcp/mod.rs`
- Test: `tests/mcp_prompts.rs`

**Interfaces:**
- Consumes: Python prompt fixture and RMCP prompt model types.
- Produces: `prompts/list` and `prompts/get` with five compatible prompt names and arguments.

- [ ] **Step 1: Write failing prompt list, default, and correctness tests**

Create `tests/mcp_prompts.rs` and assert all five names, descriptions, and these argument contracts:

| Prompt | Required | Optional/default |
|---|---|---|
| `analyze-stock` | `ticker` | `include_news=true` |
| `compare-stocks` | `ticker1`, `ticker2` | `period="3 months"` |
| `crypto-market-overview` | none | `tickers="btcusd,ethusd,solusd"` |
| `earnings-report-analysis` | `ticker`, `earnings_date` | none |
| `forex-pair-analysis` | `pair` | `period="1 month"` |

Also assert: analyze-stock includes `get_company_meta` before requesting sector; earnings text contains `expectations data is not available` and does not instruct the model to infer a beat/miss from trends; forex text describes unexplained moves without assigning likely causes.

- [ ] **Step 2: Run and confirm prompts are absent**

Run: `cargo test --test mcp_prompts`

Expected: FAIL because the server does not advertise prompts.

- [ ] **Step 3: Implement prompt metadata and strict argument extraction**

Create `src/mcp/prompts.rs` with exact metadata and argument helpers:

```rust
use rmcp::{ErrorData, model::{GetPromptRequestParams, GetPromptResult, Prompt, PromptArgument, PromptMessage, Role}};
use serde_json::{Map, Value};

fn argument(name: &str, description: &str, required: bool) -> PromptArgument {
    PromptArgument::new(name).with_description(description).with_required(required)
}

pub fn list() -> Vec<Prompt> {
    vec![
        Prompt::new("analyze-stock", Some("Comprehensive single-stock analysis: metadata, prices, fundamentals, and news"), Some(vec![argument("ticker", "Stock ticker symbol", true), argument("include_news", "Whether to include recent news; defaults to true", false)])),
        Prompt::new("compare-stocks", Some("Side-by-side comparison of two stocks: prices, fundamentals, and performance"), Some(vec![argument("ticker1", "First stock ticker", true), argument("ticker2", "Second stock ticker", true), argument("period", "Comparison period; defaults to 3 months", false)])),
        Prompt::new("crypto-market-overview", Some("Crypto market snapshot: current prices, 24h changes, and 7-day trends"), Some(vec![argument("tickers", "Comma-separated crypto tickers; defaults to btcusd,ethusd,solusd", false)])),
        Prompt::new("earnings-report-analysis", Some("Analyze a stock's earnings report: financials, price reaction, and news sentiment"), Some(vec![argument("ticker", "Stock ticker symbol", true), argument("earnings_date", "Earnings date in YYYY-MM-DD format", true)])),
        Prompt::new("forex-pair-analysis", Some("Currency pair analysis: current rate, historical trend, and volatility"), Some(vec![argument("pair", "Lowercase currency pair", true), argument("period", "Analysis period; defaults to 1 month", false)])),
    ]
}

fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String, ErrorData> {
    match arguments.get(name).and_then(Value::as_str).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value.to_owned()),
        None => Err(ErrorData::invalid_params(format!("{name} must be a non-empty string"), None)),
    }
}

fn optional_string(arguments: &Map<String, Value>, name: &str, default: &str) -> Result<String, ErrorData> {
    match arguments.get(name) {
        None => Ok(default.to_owned()),
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(ErrorData::invalid_params(format!("{name} must be a non-empty string"), None)),
    }
}

fn optional_bool(arguments: &Map<String, Value>, name: &str, default: bool) -> Result<bool, ErrorData> {
    match arguments.get(name) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::String(value)) if value == "true" => Ok(true),
        Some(Value::String(value)) if value == "false" => Ok(false),
        _ => Err(ErrorData::invalid_params(format!("{name} must be true or false"), None)),
    }
}
```

Required strings reject missing, non-string, or empty values. Unknown names return `ErrorData::invalid_params("prompt not found", None)`.

- [ ] **Step 4: Port each prompt with the approved wording corrections**

Add `get` with the complete corrected prompt bodies:

```rust
pub fn get(request: GetPromptRequestParams) -> Result<GetPromptResult, ErrorData> {
    let arguments = request.arguments.unwrap_or_default();
    let text = match request.name.as_str() {
        "analyze-stock" => {
            let ticker = required_string(&arguments, "ticker")?;
            let include_news = optional_bool(&arguments, "include_news", true)?;
            let news = if include_news {
                format!("5. Call get_news with tickers={ticker} to fetch recent news articles and identify reported catalysts and market-moving events.\n")
            } else {
                "5. Skip news fetching because include_news is false.\n".to_owned()
            };
            format!(
                "Please perform a comprehensive analysis of {ticker} using these steps:\n\n\
                 1. Call get_stock_metadata for {ticker} to retrieve company name, exchange, description, and available date range.\n\
                 2. Call get_company_meta with tickers={ticker} to retrieve sector and industry.\n\
                 3. Call get_stock_prices for {ticker} with a start_date 30 days ago to get recent end-of-day OHLCV history.\n\
                 4. Call get_daily_fundamentals for {ticker} with a start_date 30 days ago to retrieve valuation metrics.\n\
                 {news}\n\
                 Synthesize the results into these sections:\n\
                 - **Company Overview**: Name, exchange, sector, industry, and business description.\n\
                 - **Price Trend**: Recent price action, highs/lows, and percentage change over 30 days.\n\
                 - **Valuation Snapshot**: Current P/E ratio, market cap, and notable fundamental metrics.\n\
                 - **Recent Catalysts**: Reported news or events if news was fetched; do not infer causes absent evidence.\n\
                 - **Summary**: One-paragraph narrative combining the retrieved findings."
            )
        }
        "compare-stocks" => {
            let ticker1 = required_string(&arguments, "ticker1")?;
            let ticker2 = required_string(&arguments, "ticker2")?;
            let period = optional_string(&arguments, "period", "3 months")?;
            format!(
                "Please compare {ticker1} and {ticker2} over the past {period}.\n\n\
                 1. Call get_stock_prices for both tickers covering the period.\n\
                 2. Call get_daily_fundamentals for both tickers to retrieve P/E ratios and market caps.\n\
                 3. Call get_dividend_yield for both tickers.\n\n\
                 Report a comparison table for price performance, P/E, market cap, and dividend yield; then discuss momentum, relative valuation, income, and a clearly qualified conclusion based only on the retrieved data."
            )
        }
        "crypto-market-overview" => {
            let tickers = optional_string(&arguments, "tickers", "btcusd,ethusd,solusd")?;
            format!(
                "Provide a crypto market overview for {tickers}.\n\n\
                 1. Call get_crypto_quote with tickers={tickers} for current prices and available current fields.\n\
                 2. Call get_crypto_prices for {tickers} with a start_date 7 days ago for trend analysis.\n\n\
                 Report **Current Prices**, **24h Change** when supported by returned observations, **7-Day Trend**, and a **Market Narrative** grounded in the retrieved price data."
            )
        }
        "earnings-report-analysis" => {
            let ticker = required_string(&arguments, "ticker")?;
            let earnings_date = required_string(&arguments, "earnings_date")?;
            format!(
                "Analyze the earnings report for {ticker} around {earnings_date}.\n\n\
                 1. Call get_financial_statements for {ticker} across approximately three months before and after {earnings_date}.\n\
                 2. Call get_stock_prices from two weeks before through two weeks after {earnings_date}.\n\
                 3. Call get_news with tickers={ticker} from one week before through one week after {earnings_date}.\n\n\
                 Report **Financial Results**, **Price Reaction**, **News Sentiment**, and **Outlook**. Expectations data is not available from this server, so do not label the results a beat or miss unless an article supplies an explicit consensus comparison."
            )
        }
        "forex-pair-analysis" => {
            let pair = required_string(&arguments, "pair")?;
            let period = optional_string(&arguments, "period", "1 month")?;
            format!(
                "Analyze {pair} over the past {period}.\n\n\
                 1. Call get_forex_quote for {pair} for the current bid, ask, and mid price.\n\
                 2. Call get_forex_prices for {pair} with a start_date {period} ago and resample_freq='1day'.\n\n\
                 Report **Current Rate**, **Trend over {period}**, **Volatility**, **Notable Moves**, and **Summary**. Describe statistically notable spikes or drops, but state that price data alone cannot establish their cause."
            )
        }
        _ => return Err(ErrorData::invalid_params("prompt not found", None)),
    };
    Ok(GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]))
}
```

The `include_news=false` branch above deliberately contains no `get_news` instruction.

- [ ] **Step 5: Advertise and serve prompts**

Extend `ServerCapabilities::builder()` with `.enable_prompts()` and add these methods to the existing `#[tool_handler] impl ServerHandler for TiingoServer`:

```rust
async fn list_prompts(
    &self,
    _request: Option<rmcp::model::PaginatedRequestParams>,
    _context: rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<rmcp::model::ListPromptsResult, rmcp::ErrorData> {
    Ok(rmcp::model::ListPromptsResult::with_all_items(prompts::list()))
}

async fn get_prompt(
    &self,
    request: rmcp::model::GetPromptRequestParams,
    _context: rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<rmcp::model::GetPromptResponse, rmcp::ErrorData> {
    Ok(prompts::get(request)?.into())
}
```

- [ ] **Step 6: Pass all prompt, resource, and tool tests**

Run:

```bash
cargo test --test mcp_prompts --test mcp_resources --test mcp_tools
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all tests pass and the server advertises tools, resources, and prompts together.

- [ ] **Step 7: Commit prompt parity and corrections**

```bash
git add src/mcp/prompts.rs src/mcp/mod.rs tests/mcp_prompts.rs
git commit -m "feat: port corrected MCP prompts"
```

---

### Task 9: Prove canonical MCP parity through a real stdio child process

**Files:**
- Create: `tests/mcp_contract.rs`
- Create: `tests/stdio_process.rs`
- Create: `tests/live_smoke.rs`
- Modify: `Cargo.toml` only to keep the already-declared RMCP client and child-process dev features locked

**Interfaces:**
- Consumes: frozen Python contract and complete Rust server.
- Produces: version-negotiated discovery, child-process lifecycle, stdout purity, intentional-delta, and optional live evidence required before Python deletion.

- [ ] **Step 1: Write the child-process discovery contract test**

Use RMCP's `TokioChildProcess` with `env!("CARGO_BIN_EXE_tiingo-mcp")`:

```rust
use rmcp::{ServiceExt, transport::TokioChildProcess};
use tokio::process::Command;

#[tokio::test]
async fn child_process_exposes_the_complete_contract() -> anyhow::Result<()> {
    let transport = TokioChildProcess::new(Command::new(env!("CARGO_BIN_EXE_tiingo-mcp")))?;
    let client = ().serve(transport).await?;
    assert_eq!(client.list_all_tools().await?.len(), 17);
    assert_eq!(client.list_all_resources().await?.len(), 3);
    assert_eq!(client.list_all_resource_templates().await?.len(), 1);
    assert_eq!(client.list_all_prompts().await?.len(), 5);
    client.cancel().await?;
    Ok(())
}
```

- [ ] **Step 2: Add canonical comparison against the Python fixture**

In `tests/mcp_contract.rs`, parse `python-mcp.json` with `include_str!`, collect and sort names/URIs/argument names from both servers, and assert equality. Add explicit assertions for every approved delta rather than rewriting the Python fixture. Ignore JSON object key order, implementation version, current-protocol `resultType`, cache metadata, and the additive structured output fields.

- [ ] **Step 3: Add protocol and stdout lifecycle tests**

In `tests/stdio_process.rs`, assert:

- legacy `initialize` negotiation succeeds for the Python-compatible protocol version;
- current `server/discover` advertises the RMCP-supported versions and tools/resources/prompts;
- closing stdin terminates the binary within five seconds;
- setting `RUST_LOG=debug` writes diagnostics to stderr and never places a tracing line on stdout;
- `--help` and `--version` exit without starting MCP.

Use the pinned RMCP types for the two protocol probes:

```rust
#[derive(Debug, Clone)]
struct VersionedClient(rmcp::model::ProtocolVersion);

impl rmcp::ClientHandler for VersionedClient {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        let mut info = rmcp::model::ClientInfo::default();
        info.protocol_version = self.0.clone();
        info
    }
}

#[tokio::test]
async fn stdio_supports_python_compatible_initialize_and_current_discover() -> anyhow::Result<()> {
    let transport = rmcp::transport::TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_tiingo-mcp")),
    )?;
    let client = VersionedClient(rmcp::model::ProtocolVersion::V_2025_11_25)
        .serve(transport)
        .await?;
    assert_eq!(
        client.peer_info().expect("initialize peer info").protocol_version,
        rmcp::model::ProtocolVersion::V_2025_11_25,
    );

    let mut meta = rmcp::model::RequestMetaObject::new();
    meta.set_protocol_version(rmcp::model::ProtocolVersion::V_2026_07_28);
    meta.set_client_info(rmcp::model::Implementation::new("contract-test", "2.0.0"));
    meta.set_client_capabilities(rmcp::model::ClientCapabilities::default());
    let discovery = client.discover(meta).await?;
    assert!(discovery.supported_versions.contains(&rmcp::model::ProtocolVersion::V_2025_11_25));
    assert!(discovery.supported_versions.contains(&rmcp::model::ProtocolVersion::V_2026_07_28));
    assert!(discovery.capabilities.tools.is_some());
    assert!(discovery.capabilities.resources.is_some());
    assert!(discovery.capabilities.prompts.is_some());
    client.cancel().await?;
    Ok(())
}
```

Import `rmcp::{ClientHandler, ServiceExt}` so `.serve()` resolves. Use `assert_cmd` plus `wait_timeout` for the EOF, stdout, help, and version cases; add `wait-timeout = "0.2"` to dev dependencies in Task 2 rather than implementing a sleep loop.

- [ ] **Step 4: Add an ignored, quota-bounded live smoke**

Create `tests/live_smoke.rs` with `#[ignore = "requires TIINGO_API_KEY and consumes quota"]` tests for stock metadata, one short EOD range, one forex pair, one filtered crypto price call, one filtered news call, fundamentals definitions, and one corporate-action request. For every family, classify 403 separately as `Entitlement` evidence and print the capability; fail on authentication, route, decoding, or other unexpected errors. Require at least stock metadata and the short EOD range to succeed so a credential with no usable baseline access cannot produce a false green run.

- [ ] **Step 5: Run offline contract verification**

Run:

```bash
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Expected: every offline test passes.

- [ ] **Step 6: Run the authorized live smoke when a key is present**

Run: `cargo test --test live_smoke -- --ignored --nocapture --test-threads=1`

Expected: stock metadata and EOD pass; each other product family either passes or reports a distinct current entitlement outcome.

- [ ] **Step 7: Commit full MCP parity evidence**

```bash
git add Cargo.toml Cargo.lock tests/mcp_contract.rs tests/stdio_process.rs tests/live_smoke.rs
git commit -m "test: prove Rust MCP parity"
```

---

### Task 10: Measure cold start, RSS, and local wrapper overhead

**Files:**
- Create: `examples/migration_probe.rs`
- Create: `scripts/benchmark_python_wrapper.py`
- Create: `docs/performance/2026-08-24-rust-cutover.md`
- Modify: `Cargo.toml` only to keep the already-declared `sysinfo` dev dependency locked

**Interfaces:**
- Consumes: runnable Python and Rust stdio servers before cutover.
- Produces: repeatable measurements and a committed report proving the performance gate.

- [ ] **Step 1: Add a failing probe smoke check**

Run: `cargo run --release --example migration_probe -- --runs 1 -- uv run tiingo-mcp`

Expected: FAIL because the example does not exist.

- [ ] **Step 2: Implement the process probe**

Create `examples/migration_probe.rs` that:

- parses `--runs N -- <command> [args]`;
- spawns the command with piped stdin/stdout and inherited stderr suppression;
- writes one newline-delimited JSON-RPC `initialize` request using protocol `2025-06-18`;
- measures `Instant::now()` to the first complete response line;
- refreshes the child with `sysinfo::System` and records RSS bytes after initialization;
- sends `tools/list`, `resources/list`, reads all three fixed resources, and gets all five prompt/default cases, then records RSS again as the same fixture workload for both runtimes;
- closes stdin and enforces a five-second exit bound;
- repeats exactly `N` times;
- prints one JSON object containing `runs`, `startup_ms_median`, `startup_ms_p95`, `rss_initialized_bytes_median`, and `rss_workload_bytes_median`.

Use deterministic percentile selection after sorting: median index `(n - 1) / 2`, p95 index `ceil(0.95 * n) - 1`.

- [ ] **Step 3: Add the temporary Python local-wrapper benchmark**

Create `scripts/benchmark_python_wrapper.py`:

```python
from __future__ import annotations

import asyncio
import json
import math
import time

import httpx
from fastmcp import Client

import tiingo_mcp.server as srv
from tiingo_mcp.client import TiingoClient
from tiingo_mcp.server import mcp

RUNS = 1_000


async def main() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.host == "fixture.invalid"
        return httpx.Response(200, json={"ticker": "AAPL"})

    upstream = TiingoClient(api_key="benchmark-key")
    await upstream._client.aclose()
    upstream._client = httpx.AsyncClient(
        base_url="https://fixture.invalid",
        headers={"Authorization": "Token benchmark-key"},
        transport=httpx.MockTransport(handler),
    )
    srv._client = upstream
    samples: list[float] = []
    try:
        async with Client(mcp) as client:
            for _ in range(10):
                await client.call_tool("get_stock_metadata", {"ticker": "AAPL"})
            for _ in range(RUNS):
                started = time.perf_counter_ns()
                result = await client.call_tool("get_stock_metadata", {"ticker": "AAPL"})
                samples.append((time.perf_counter_ns() - started) / 1_000)
                assert not result.is_error
    finally:
        srv._client = None
        await upstream.close()

    samples.sort()
    print(json.dumps({
        "runs": RUNS,
        "wrapper_us_median": samples[(RUNS - 1) // 2],
        "wrapper_us_p95": samples[math.ceil(0.95 * RUNS) - 1],
    }, sort_keys=True))


if __name__ == "__main__":
    asyncio.run(main())
```

The fixed hostname assertion proves the script never calls Tiingo.

- [ ] **Step 4: Add the matching Rust local-wrapper benchmark mode**

Add `--wrapper-runs 1000` to `migration_probe`. That mode starts an internal Wiremock server returning `{"ticker":"AAPL"}`, builds `TiingoClient` with a `Config` pointing to that URL, opens one in-process RMCP client/server pair, calls `get_stock_metadata` exactly 1,000 times, excludes setup/teardown from timing, and prints `{"runs":1000,"wrapper_us_median":number,"wrapper_us_p95":number}`. It never reads `TIINGO_API_KEY` or contacts Tiingo.

- [ ] **Step 5: Build release mode and collect 30-run evidence on one machine**

Run:

```bash
cargo build --release
cargo run --release --example migration_probe -- --runs 30 -- uv run tiingo-mcp
cargo run --release --example migration_probe -- --runs 30 -- target/release/tiingo-mcp
uv run python scripts/benchmark_python_wrapper.py
cargo run --release --example migration_probe -- --wrapper-runs 1000
```

Record the exact machine/OS, Rust/Python/FastMCP/RMCP versions, raw command outputs, percentage differences, largest captured fixture in bytes, its margin below the 8 MiB cap, and the explicit statement that Tiingo network latency was excluded in `docs/performance/2026-08-24-rust-cutover.md`.

- [ ] **Step 6: Enforce the performance gate**

Expected: Rust median cold start, initialized RSS, and post-workload RSS are lower than Python; Rust local wrapper p95 is no worse than Python. If any predicate fails, stop the cutover, retain Python, and diagnose before Task 11.

- [ ] **Step 7: Commit performance evidence and repeatable probes**

```bash
git add Cargo.toml Cargo.lock examples/migration_probe.rs scripts/benchmark_python_wrapper.py docs/performance/2026-08-24-rust-cutover.md
git commit -m "perf: verify Rust migration gains"
```

---

### Task 11: Add Rust CI, native releases, and path-free MCPB packaging

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `dist-workspace.toml`
- Generate: `.github/workflows/release.yml`
- Create: `deny.toml`
- Create: `packaging/mcpb/manifest.json`
- Create: `packaging/mcpb/package.sh`
- Test: release and MCPB dry runs

**Interfaces:**
- Consumes: fully verified Rust package and binary.
- Produces: CI, five target archives, checksums/attestations/installers, and validated MCPB files for Task 12 documentation.

- [ ] **Step 1: Replace CI with the full offline Rust gate**

Make `.github/workflows/ci.yml` run on pull requests and `main` pushes with:

```yaml
- run: cargo fmt --check
- run: cargo clippy --all-targets --all-features -- -D warnings
- run: cargo test --all-targets --locked
- run: cargo build --release --locked
- run: cargo deny check
```

Use a matrix containing Rust `1.88.0` and `stable`; run formatting and `cargo deny` once on stable. Do not place `TIINGO_API_KEY` in ordinary CI.

Add a separate native-artifact matrix that builds and runs `tiingo-mcp --version` on each matching hosted architecture:

| Runner | Target | Build command |
|---|---|---|
| `macos-14` | `aarch64-apple-darwin` | `cargo build --release --locked --target aarch64-apple-darwin` |
| `macos-13` | `x86_64-apple-darwin` | `cargo build --release --locked --target x86_64-apple-darwin` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-musl` | `cargo zigbuild --release --locked --target aarch64-unknown-linux-musl` |
| `ubuntu-24.04` | `x86_64-unknown-linux-musl` | `cargo zigbuild --release --locked --target x86_64-unknown-linux-musl` |
| `windows-2025` | `x86_64-pc-windows-msvc` | `cargo build --release --locked --target x86_64-pc-windows-msvc` |

The Linux jobs install pinned `cargo-zigbuild` and Zig versions. Every job runs the target binary with `--version`, builds its MCPB, validates that bundle with MCPB CLI 2.1.2, and uploads the archive plus bundle as non-release CI artifacts. A missing hosted runner is a blocker requiring a replacement runner decision, not permission to drop a target.

- [ ] **Step 2: Add dependency, license, and source policy**

Create `deny.toml` allowing MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, and Zlib; deny known vulnerabilities, unmaintained/yanked crates, unknown registries, and git dependencies. Run `cargo deny check` and document any transitive license exception with crate name and license text rather than using a wildcard.

- [ ] **Step 3: Verify the crates.io package identity without publishing**

Run:

```bash
curl --fail-with-body --silent --show-error https://crates.io/api/v1/crates/tiingo-mcp
cargo package --locked --list
cargo publish --dry-run --locked
```

Expected: the crates.io request returns 404 before first publication, the package list contains only intended source/docs/license files, and the publish dry run succeeds without uploading. If the name exists by execution time, stop and request a package-name decision; do not silently rename the binary or crate.

- [ ] **Step 4: Configure cargo-dist 0.32.0**

Create `dist-workspace.toml`:

```toml
[workspace]
members = ["cargo:."]

[dist]
cargo-dist-version = "0.32.0"
ci = "github"
installers = ["shell", "powershell"]
targets = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "aarch64-unknown-linux-musl",
  "x86_64-unknown-linux-musl",
  "x86_64-pc-windows-msvc",
]
hosting = "github"
github-attestations = true
install-path = "CARGO_HOME"
install-updater = false
```

Install cargo-dist 0.32.0 in the execution environment, run `dist init --yes`, and commit the generated `.github/workflows/release.yml`. Inspect the generated workflow to verify tag-only release behavior, checksums, GitHub artifact attestations, and all five targets.

- [ ] **Step 5: Add a binary MCPB manifest with sensitive key configuration**

Create `packaging/mcpb/manifest.json`:

```json
{
  "manifest_version": "0.3",
  "name": "tiingo-mcp",
  "display_name": "Tiingo MCP",
  "version": "2.0.0",
  "description": "Stocks, forex, crypto, news, fundamentals, and corporate actions from Tiingo.",
  "author": { "name": "Major7" },
  "repository": { "type": "git", "url": "https://github.com/major7apps/tiingo-mcp" },
  "license": "MIT",
  "server": {
    "type": "binary",
    "entry_point": "server/tiingo-mcp",
    "mcp_config": {
      "command": "server/tiingo-mcp",
      "args": [],
      "env": { "TIINGO_API_KEY": "${user_config.tiingo_api_key}" },
      "platform_overrides": {
        "win32": { "command": "server/tiingo-mcp.exe" }
      }
    }
  },
  "user_config": {
    "tiingo_api_key": {
      "type": "string",
      "title": "Tiingo API key",
      "description": "API key issued by Tiingo",
      "sensitive": true,
      "required": true
    }
  },
  "compatibility": { "platforms": ["darwin", "linux", "win32"] }
}
```

- [ ] **Step 6: Add deterministic per-target MCPB packaging**

Create `packaging/mcpb/package.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 2 ]]; then
  echo "usage: $0 <target-triple> <binary-directory>" >&2
  exit 64
fi

target="$1"
binary_directory="$2"
case "$target" in
  *-apple-darwin) platform="darwin"; executable="tiingo-mcp" ;;
  *-unknown-linux-*) platform="linux"; executable="tiingo-mcp" ;;
  *-pc-windows-*) platform="win32"; executable="tiingo-mcp.exe" ;;
  *) echo "unsupported target: $target" >&2; exit 65 ;;
esac
binary="$binary_directory/$executable"

if [[ ! -f "$binary" ]]; then
  echo "binary not found: $binary" >&2
  exit 66
fi

stage="target/mcpb/$target"
bundle="target/distrib/tiingo-mcp-$target.mcpb"
rm -rf -- "$stage"
mkdir -p "$stage/server" target/distrib
cp packaging/mcpb/manifest.json "$stage/manifest.json"
cp "$binary" "$stage/server/$executable"
chmod +x "$stage/server/$executable" 2>/dev/null || true
jq --arg platform "$platform" '.compatibility.platforms = [$platform]' \
  "$stage/manifest.json" > "$stage/manifest.tmp.json"
mv "$stage/manifest.tmp.json" "$stage/manifest.json"
npx --yes @anthropic-ai/mcpb@2.1.2 pack "$stage" "$bundle"
```

The only recursive removal is the validated repository-relative `target/mcpb/$target` staging directory, where `target` has first matched one of the three closed target patterns.

- [ ] **Step 7: Dry-run native and MCPB packaging without publishing**

Run on the current host target:

```bash
cargo build --release --locked
bash packaging/mcpb/package.sh aarch64-apple-darwin target/release
npx --yes @anthropic-ai/mcpb@2.1.2 validate target/distrib/tiingo-mcp-aarch64-apple-darwin.mcpb
dist plan
```

Expected: bundle validation passes; `dist plan` includes the five targets and no publish occurs.

- [ ] **Step 8: Commit distribution automation**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml dist-workspace.toml deny.toml packaging/mcpb
git commit -m "build: add native and MCPB distribution"
```

- [ ] **Step 9: Obtain green clean-target CI evidence**

Stop and request authorization to push the implementation branch if it has not already been granted. Once authorized, push only that branch and wait for the CI workflow. Record the run URL and exact result for all five native-artifact jobs. Do not start Task 12 unless every job builds, runs `--version`, and validates its MCPB.

---

### Task 12: Cut over documentation and remove every Python artifact

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `CLAUDE.md`
- Modify: `.gitignore`
- Delete: `smithery.yaml`
- Delete: `pyproject.toml`
- Delete: `uv.lock`
- Delete: `src/tiingo_mcp/**`
- Delete: all `tests/*.py`
- Delete: `scripts/capture_python_contract.py`
- Delete: `scripts/benchmark_python_wrapper.py`
- Delete: `scripts/export_rust_resources.py`
- Delete locally: `/Users/wshobson/workspace/tiingo-mcp/.venv`
- Keep: `tests/contract/baseline/python-mcp.json`
- Keep: all Rust probes, tests, source, specs, plans, research, and performance report

**Interfaces:**
- Consumes: every passing gate from Tasks 1–11.
- Produces: the final Rust-only repository, native launch documentation, and release-ready 2.0.0 tree.

- [ ] **Step 1: Re-run every pre-deletion gate and stop on the first failure**

First resolve and verify the CI run for the exact implementation commit:

```bash
tiingo_cutover_sha="$(git rev-parse HEAD)"
tiingo_ci_run_id="$(gh run list --commit "$tiingo_cutover_sha" --workflow ci.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
test -n "$tiingo_ci_run_id"
gh run view "$tiingo_ci_run_id" --json headSha,conclusion,jobs
```

The returned `headSha` must equal `tiingo_cutover_sha`, the conclusion must be `success`, and all five native-artifact jobs must be present.

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
cargo deny check
uv run pytest -v
git diff --check
```

Expected: all Rust gates pass; the frozen Python suite still passes. Do not delete Python if any command fails.

- [ ] **Step 2: Rewrite public installation and MCP launch documentation**

Update `README.md` to show:

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

Document the cargo-dist shell/PowerShell installers, `cargo install tiingo-mcp --locked`, MCPB desktop installation, direct `tiingo-mcp` stdio behavior, `cargo test --all-targets`, and the five supported targets. Remove PyPI/Python/FastMCP/`uvx` badges and commands, stale rate tables, static plan guarantees, and the false HTTP-transport claim. Use the repository URL `https://github.com/major7apps/tiingo-mcp` everywhere.

- [ ] **Step 3: Update changelog and repository guidance**

Add `## 2.0.0 (Unreleased)` to `CHANGELOG.md` covering the Rust/RMCP replacement, direct binary launch, MCPB/native distribution, structured results, error corrections, current crypto route, prompt/resource corrections, and the deliberate removal of `uvx`. Rewrite `CLAUDE.md` with only Rust commands and the new module boundaries.

- [ ] **Step 4: Replace Python ignores with Rust/build staging ignores**

Make `.gitignore` contain:

```gitignore
/target/
**/*.rs.bk
.env
.env.local
.idea/
.vscode/
*.swp
*.swo
*~
.DS_Store
Thumbs.db
```

Do not ignore `Cargo.lock`; this is an application binary.

- [ ] **Step 5: Delete tracked Python runtime and packaging files**

Delete exactly:

```text
pyproject.toml
uv.lock
smithery.yaml
src/tiingo_mcp/
tests/__init__.py
tests/conftest.py
tests/test_client.py
tests/test_contract_baseline.py
tests/test_integration.py
tests/test_prompts.py
tests/test_resources.py
tests/test_server.py
scripts/capture_python_contract.py
scripts/benchmark_python_wrapper.py
scripts/export_rust_resources.py
```

Preserve `tests/contract/baseline/python-mcp.json` as the historical parity oracle.

- [ ] **Step 6: Remove the local virtual environment last**

Verify the exact target first:

```bash
test -d /Users/wshobson/workspace/tiingo-mcp/.venv
```

If present, remove only `/Users/wshobson/workspace/tiingo-mcp/.venv`. Report that it was untracked and can be recreated only from the historical Python tag. Do not use a variable, glob, home-directory shortcut, or repository-wide target.

- [ ] **Step 7: Prove the repository contains no Python runtime artifact**

Run:

```bash
rg --files | rg '\.(py|pyi)$|(^|/)(pyproject\.toml|uv\.lock)$'
rg -n 'uvx|pip install tiingo-mcp|Python 3|FastMCP' README.md CLAUDE.md .github src packaging Cargo.toml
git status --short
```

Expected: both `rg` commands print no matches. `git status` shows only the intentional cutover changes.

- [ ] **Step 8: Run final Rust-only acceptance from the cleaned tree**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
cargo deny check
cargo run --quiet -- --version
cargo run --quiet < /dev/null
dist plan
git diff --check
```

Expected: every command passes; version prints `tiingo-mcp 2.0.0`; EOF invocation prints nothing to stdout; release planning does not publish.

- [ ] **Step 9: Commit the verified Rust-only cutover**

```bash
git add --all
git commit -m "feat!: cut over tiingo-mcp to Rust"
```

Commit body:

```text
BREAKING CHANGE: tiingo-mcp is now a native RMCP binary. The uvx/PyPI
installation contract has been removed; the MCP command is tiingo-mcp.
```

- [ ] **Step 10: Re-run clean-target CI at the Rust-only cutover commit**

If the Task 11 branch-push authorization covered subsequent commits to the same implementation branch, push that branch; otherwise request authorization again. Wait for `ci.yml`, then use the exact-SHA lookup from Step 1 to verify the final cutover commit and all five native-artifact jobs are green. Do not tag or publish.

- [ ] **Step 11: Hand off release readiness without publishing**

Report the final commit hash, full test counts, live-smoke entitlement outcomes, performance comparison, supported artifact matrix, MCPB validation, and that Python plus `.venv` were removed. Request explicit authorization before creating/pushing `v2.0.0`, publishing crates.io, uploading GitHub releases, or publishing the Smithery MCPB.

---

## Final Review Checklist

- [ ] Every spec section maps to a task above.
- [ ] All 17 tools have exact route and schema coverage.
- [ ] All three fixed resources, the guide template, and five prompts have identifier and content tests.
- [ ] The crypto, error, retry, symbology, resource-fact, and prompt corrections are explicit tests.
- [ ] Direct `command: "tiingo-mcp"`, stdout purity, EOF shutdown, and path-free MCPB installation are verified.
- [ ] Streamable HTTP, new REST families, and WebSockets remain absent.
- [ ] Python is retained through comparison and removed only after every gate.
- [ ] No tag, push, crate publish, GitHub release, or Smithery publish occurs without separate authorization.
