# AGENTS.md

This is the canonical repository guide for every coding agent and harness working on `tiingo-mcp`. `CLAUDE.md` is a symlink to this file.

## Project map

Following [OpenAI Harness Engineering](https://openai.com/index/harness-engineering/), this file is a concise map; use the focused references for durable detail.

- [ARCHITECTURE.md](ARCHITECTURE.md) — module boundaries, REST safety, upstream WebSocket lifecycle, and process ownership.
- [API_SURFACE.md](API_SURFACE.md) — exact tool-to-route/access/test matrix and excluded surfaces.
- [QUALITY.md](QUALITY.md) — local/CI gates, coverage, deterministic/live evidence, and documentation contracts.
- [README.md](README.md) — installation, MCP configuration, tools, entitlements, resources, and prompts.
- [CHANGELOG.md](CHANGELOG.md) — released and unreleased user-facing changes.

## Public contract

- Expose 38 tools while preserving the frozen descriptors of the original 17 tools.
- Preserve three fixed resources, one resource template, five prompts, stdio transport, JSON text compatibility, structured content, and `TIINGO_API_KEY` authentication.
- Tiingo WebSockets are upstream data connections exposed through four finite tools. Do not describe or implement an MCP WebSocket or Streamable HTTP transport.

## Operating rules

- Keep the implementation Rust-only. Do not reintroduce Python packaging, virtual environments, or `uvx` compatibility.
- Keep stdout protocol-only. Send diagnostics to stderr and never log credentials or authorization headers.
- Make surgical changes and preserve unrelated user work. Inspect `git status` before editing.
- Do not run ignored, quota-consuming live tests without explicit authorization.
- Do not publish crates, create or push tags, create releases, or upload MCPB bundles without explicit authorization.

## Commands

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo llvm-cov --all-targets --all-features --locked --fail-under-lines 93 --summary-only
cargo build --release --locked
cargo deny check all
cargo run --quiet -- --version
dist plan
```

The normal suite binds local mock servers and is credential-free. Ignored live tests consume quota or bandwidth and require explicit authorization plus `TIINGO_API_KEY`.

To build an MCPB bundle from an existing target binary directory:

```bash
bash packaging/mcpb/package.sh <target-triple> <binary-directory>
```

## Release preparation

- Keep the version synchronized in `Cargo.toml`, the root `tiingo-mcp` entry in `Cargo.lock`, and `packaging/mcpb/manifest.json`.
- Add the matching `CHANGELOG.md` section as `X.Y.Z (Unreleased)` while versioned release work accumulates. For this unversioned expansion, keep the top-level `Unreleased` section until a version is authorized.
- Before creating a release tag, replace the versioned `Unreleased` marker with the release date.
- Keep current-release URLs in `README.md` pointed at the latest published tag until its replacement exists.
- Keep GitHub Release titles derived from the tag as bare `X.Y.Z`, with no supported namespace, date, or timestamp.
- Run the [QUALITY.md](QUALITY.md) release gate plus `dist plan` and `cargo publish --dry-run --locked` before requesting release authorization.
