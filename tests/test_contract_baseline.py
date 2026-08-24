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
            "get_stock_metadata",
            "get_stock_prices",
            "get_realtime_price",
            "get_intraday_prices",
            "get_forex_quote",
            "get_forex_prices",
            "get_crypto_quote",
            "get_crypto_prices",
            "get_crypto_metadata",
            "get_news",
            "get_fundamentals_definitions",
            "get_financial_statements",
            "get_daily_fundamentals",
            "get_company_meta",
            "get_dividends",
            "get_dividend_yield",
            "get_splits",
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
