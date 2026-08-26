# Architecture

`tiingo-mcp` is a native Rust server built on the official [RMCP SDK](https://github.com/modelcontextprotocol/rust-sdk). This server exposes MCP over stdio only. Tiingo REST and WebSocket connections are upstream data transports; they do not add an MCP WebSocket or Streamable HTTP transport.

## Dependency map

```text
src/main.rs
  -> src/lib.rs (stdio lifecycle)
     -> src/mcp/ (RMCP tools, resources, prompts)
        -> src/client/ (bounded Tiingo REST boundary)
           -> family clients (EOD, IEX, equity, BOATS, funds, ...)
        -> src/websocket/registry.rs (finite subscription ownership)
           -> registry/worker.rs (socket lifecycle and queue)
              -> websocket/protocol.rs (Tiingo frames and messages)

src/config.rs -> REST client and WebSocket registry
src/error.rs  -> REST, WebSocket, and MCP error mapping
```

Dependencies point from the protocol boundary toward focused client/runtime modules. Family clients validate inputs and map typed arguments to exact Tiingo paths and query names; they do not own authentication, retries, or response limits. The WebSocket registry owns workers; MCP tools never own sockets directly.

## Configuration and credentials

`Config::from_env` reads `TIINGO_API_KEY` once during server construction. `TiingoServer::from_env` gives the REST client and market-data registry the credential they need, then both retain it privately. REST authorization headers and upstream WebSocket subscribe frames are created only at their transport boundaries. Credentials and upstream subscription IDs are redacted from debug output, logs, retained events, MCP results, and sanitized errors.

Discovery, resources, and prompts do not require a credential. A Tiingo data call without one returns a configuration tool error.

## REST boundary

All REST families use `TiingoClient::get_bytes` through either `get_json` or `get_csv`. This one boundary:

- joins paths against the configured Tiingo origin and rejects cross-origin routes;
- adds the sensitive authorization header;
- permits no more than three total attempts for safe transient failures;
- caps `Retry-After` and exponential backoff at 30 seconds;
- streams and bounds the decoded response to 8 MiB before parsing; and
- maps 401 to authentication, 403 to entitlement, 404 to not-found, and retryable failures to sanitized MCP tool errors.

Bulk EOD refresh is the intentional CSV consumer. It parses `/tiingo/daily/prices?format=csv` into typed JSON while preserving raw and adjusted OHLCV, `divCash`, and `splitFactor`. A non-unit split factor or positive cash dividend marks the ticker for a full historical reseed from `/tiingo/daily/{ticker}/prices`.

## Upstream WebSocket lifecycle

The four MCP lifecycle tools expose finite interactions with Tiingo IEX and consolidated-equity WebSockets:

```text
start -> validate -> connect -> subscribe -> initial I acknowledgement -> active
active -> poll -> active
active -> update symbols -> acknowledgement -> active
active -> stop / expiry / data_gap / terminal error -> cancel -> close -> join
active -> transport or 75-second liveness failure -> bounded reconnect -> active/error
```

`start` accepts 1–100 explicit symbols and at most eight process-local sessions. It generates a 32-character lowercase hexadecimal local ID and waits no more than five seconds for the initial `I` acknowledgement. IEX defaults to threshold 6; levels 0 and 5 require explicit direct-agreement confirmation. Consolidated equity defaults to 6 and accepts only 4 or 6.

Each session has a 2,048-event, 8-MiB queue. The next event that would exceed either bound terminates the session as `data_gap`; data is never silently dropped. Polling uses a local arrival cursor and returns at most 256 events, 1 MiB, or five seconds of wait. Reusing a cursor replays the same retained events. Cancelling a poll cancels only that request.

Events retain vendor time and local receive time. Arrival sequence is authoritative. Duplicate and decreasing vendor timestamps are preserved and flagged as `duplicate` and `outOfOrder` rather than discarded or reordered.

Recoverable transport/liveness failures reconnect after exactly 250, 500, 1,000, 2,000, and 4,000 milliseconds. Each attempt creates a fresh connection, subscribes again, and adopts the new upstream ID. Authentication and entitlement failures do not reconnect. Heartbeats and data refresh the 75-second liveness deadline.

A session expires after 30 minutes absolute lifetime or five minutes without a registry call. Updates may add or remove symbols; changing threshold requires stop/start. Stop is idempotent and performs best-effort unsubscribe, socket close, cancellation, and join.

## Process ownership and I/O

The registry owns every worker `JoinHandle`. A failed or cancelled start cleans up its unpublished worker. Stop awaits cleanup, and `run_stdio_with` shuts down and joins the whole registry whenever initialization fails, the service is cancelled, or stdin closes.

Stdout is reserved for MCP protocol bytes. Tracing and diagnostics go to stderr. Closing stdin must terminate promptly without diagnostic or credential noise on stdout.

See [API_SURFACE.md](API_SURFACE.md) for routes and access classes and [QUALITY.md](QUALITY.md) for the tests that enforce these boundaries.
