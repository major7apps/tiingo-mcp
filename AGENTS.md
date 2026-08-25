# AGENTS.md

This is the canonical repository guide for every coding agent and harness working on `tiingo-mcp`. `CLAUDE.md` is a symlink to this file so the guidance stays identical across tools.

## Operating rules

- Keep the implementation Rust-only. Do not reintroduce Python packaging, virtual environments, or `uvx` compatibility.
- Preserve the public MCP contract unless the task explicitly changes it: 17 tools, three fixed resources, one resource template, five prompts, stdio transport, and `TIINGO_API_KEY` authentication.
- Keep stdout protocol-only. Send diagnostics to stderr and never log credentials or authorization headers.
- Make surgical changes and preserve unrelated user work. Inspect `git status` before editing.
- Do not run the quota-consuming live test without explicit authorization.
- Do not publish crates, create or push tags, create releases, or upload MCPB bundles without explicit authorization.

## Commands

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
cargo deny check
cargo run --quiet -- --version
TIINGO_API_KEY=key cargo run --quiet
dist plan
```

The test suite binds local mock HTTP servers. The ignored live-smoke test consumes quota and must be invoked deliberately with `TIINGO_API_KEY`.

To build an MCPB bundle from an existing target binary directory:

```bash
bash packaging/mcpb/package.sh <target-triple> <binary-directory>
```

## Architecture

This is a native RMCP stdio server wrapping the Tiingo financial-data REST API.

- `src/main.rs` parses the CLI, initializes stderr-only tracing, and starts the RMCP stdio service. `--help` and `--version` exit without starting MCP.
- `src/lib.rs` exports the server modules used by the binary and integration tests.
- `src/config.rs` loads `TIINGO_API_KEY` lazily so discovery, resources, and prompts work without credentials.
- `src/client/mod.rs` owns the shared `reqwest::Client`, request bounds, retry policy, same-origin enforcement, and family-specific clients.
- `src/client/{eod,iex,forex,crypto,news,fundamentals,corporate_actions}.rs` maps tool inputs to exact Tiingo routes and query parameters.
- `src/client/query.rs` serializes optional query values without sending absent fields.
- `src/error.rs` classifies configuration, validation, transport, HTTP, and response-size failures and maps them to sanitized MCP tool errors.
- `src/mcp/tools.rs` exposes the 17 typed tools and returns both legacy JSON text and structured content.
- `src/mcp/resources.rs` serves three fixed resources plus the `tiingo://guide/{asset_class}` template from embedded JSON under `src/mcp/data/`.
- `src/mcp/prompts.rs` exposes the five compatible, corrected analysis prompts.

## Runtime contracts

- `tiingo-mcp` uses stdio transport by default. Protocol output belongs on stdout; diagnostics belong on stderr.
- Closing stdin must terminate the process promptly and without stdout noise.
- The credential contract is `TIINGO_API_KEY`. Never log it or the authorization header.
- Requests must remain on the configured Tiingo origin, retry at most three total attempts, and enforce the 8 MiB decoded-response limit.
- Recoverable tool failures set MCP `isError: true` while retaining the sanitized JSON text block expected by older clients.
- Optional MCP tool inputs use closed JSON schemas and omit absent Tiingo query parameters.

## Tests

- `tests/client_http.rs` covers authentication, validation, retries, redaction, origin safety, and response bounds.
- `tests/client_market_routes.rs` and `tests/client_data_routes.rs` cover exact REST paths and query mappings.
- `tests/mcp_contract.rs` compares discovery against `tests/contract/baseline/v1-mcp.json`, the frozen v1 parity oracle, plus only the approved correctness deltas.
- `tests/mcp_tools.rs`, `tests/mcp_resources.rs`, and `tests/mcp_prompts.rs` cover the RMCP surface.
- `tests/stdio_process.rs` covers negotiation, stdout purity, CLI behavior, and EOF shutdown in a child process.
- `tests/live_smoke.rs` keeps offline validators enabled and the quota-consuming live test ignored by default.

## Distribution

`cargo-dist` produces shell and PowerShell installers and native artifacts for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-musl`
- `x86_64-unknown-linux-musl`
- `x86_64-pc-windows-msvc`

Each native CI job builds and validates an MCPB 0.3 bundle. Release automation is tag-triggered.

## Release preparation

- Keep the version synchronized in `Cargo.toml`, the root `tiingo-mcp` entry in `Cargo.lock`, and `packaging/mcpb/manifest.json`.
- Add the matching `CHANGELOG.md` section as `X.Y.Z (Unreleased)` while changes are accumulating.
- Before creating the release tag, replace `Unreleased` with the release date for changelog history.
- Keep the GitHub Release title derived from the tag without a supported namespace or leading `v` so the webpage shows only `X.Y.Z`, with no date or timestamp.
- Keep current-release URLs in `README.md` pointed at the latest published tag until the replacement release exists.
- Run the full verification set plus `dist plan` and `cargo publish --dry-run --locked` before requesting release authorization.
