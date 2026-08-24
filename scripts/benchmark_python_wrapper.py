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
    print(
        json.dumps(
            {
                "runs": RUNS,
                "wrapper_us_median": samples[(RUNS - 1) // 2],
                "wrapper_us_p95": samples[math.ceil(0.95 * RUNS) - 1],
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    asyncio.run(main())
