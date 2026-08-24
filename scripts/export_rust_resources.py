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
                item
                for item in guide["common_pitfalls"]
                if not any(word in item.lower() for word in ("free tier", "paid plan"))
            ]
            if name == "stocks":
                guide = json.loads(json.dumps(guide).replace("BRK.B", "BRK-A"))
            if name == "crypto":
                guide["current_price_route"] = "/tiingo/crypto/prices"
            write(OUT / "guides" / f"{name}.json", guide)


if __name__ == "__main__":
    asyncio.run(main())
