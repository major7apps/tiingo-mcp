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
    await client.get_stock_prices(
        "AAPL", start_date="2024-01-01", end_date="2024-01-31", resample_freq="weekly"
    )
    await client.get_realtime_price("AAPL", after_hours=True)
    await client.get_intraday_prices(
        "AAPL", start_date="2024-01-01", end_date="2024-01-02", resample_freq="5min"
    )
    await client.get_forex_quote("eurusd")
    await client.get_forex_prices(
        "eurusd", start_date="2024-01-01", end_date="2024-01-02", resample_freq="1day"
    )
    await client.get_crypto_quote("btcusd")
    await client.get_crypto_prices(
        "btcusd", start_date="2024-01-01", end_date="2024-01-02", resample_freq="1hour"
    )
    await client.get_crypto_metadata("btcusd")
    await client.get_news(
        tickers="AAPL",
        tags="earnings",
        source="reuters",
        start_date="2024-01-01",
        end_date="2024-01-31",
        limit=10,
        offset=5,
        sort_by="publishedDate",
    )
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
        str(status): (
            lambda request, status=status: httpx.Response(status, text=f"status {status}")
        )
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
