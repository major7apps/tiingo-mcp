use std::{cmp::Ordering, time::Duration};

use anyhow::Context;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, GetPromptRequestParams, JsonObject, ReadResourceRequestParams},
    transport::TokioChildProcess,
};
use serde_json::{Map, Value};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, MAX_RESPONSE_BYTES, RetryPolicy},
};
use tokio::process::Command;
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const V1_CONTRACT: &str = include_str!("contract/baseline/v1-mcp.json");
const CHILD_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_DATE: &str = "2026-08-24";
const OFFICIAL_SOURCES: [&str; 2] = [
    "https://www.tiingo.com/documentation/general/overview",
    "https://api.tiingo.com/documentation/end-of-day",
];
const V1_INITIALIZE_INSTRUCTIONS: &str = "Financial data server powered by Tiingo. Provides real-time and historical stock prices, forex rates, crypto data, news, fundamentals, and corporate actions. All date parameters use YYYY-MM-DD format.";
const RUST_INITIALIZE_INSTRUCTIONS: &str =
    "Financial data server powered by Tiingo. Dates use YYYY-MM-DD.";

fn child_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tiingo-mcp"));
    command.env_remove("TIINGO_API_KEY");
    command
}

fn arguments(value: Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

fn sort_by_string_field(items: &mut [Value], field: &str) {
    items.sort_by(|left, right| {
        left[field]
            .as_str()
            .partial_cmp(&right[field].as_str())
            .unwrap_or(Ordering::Equal)
    });
}

fn normalize_schema(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(normalize_schema).collect()),
        Value::Object(mut object) => {
            for value in object.values_mut() {
                *value = normalize_schema(value.take());
            }
            if let Some(branches) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
                branches.sort_by_key(Value::to_string);
            }
            if let Some(branches) = object.get("anyOf").and_then(Value::as_array)
                && branches.iter().all(|branch| {
                    branch
                        .as_object()
                        .is_some_and(|branch| branch.len() == 1 && branch["type"].is_string())
                })
            {
                let mut types = branches
                    .iter()
                    .map(|branch| branch["type"].clone())
                    .collect::<Vec<_>>();
                types.sort_by_key(Value::to_string);
                object.remove("anyOf");
                object.insert("type".to_owned(), Value::Array(types));
            }
            for key in ["enum", "required", "type"] {
                if let Some(values) = object.get_mut(key).and_then(Value::as_array_mut) {
                    values.sort_by_key(Value::to_string);
                }
            }
            Value::Object(object)
        }
        value => value,
    }
}

fn remove_exact(object: &mut Map<String, Value>, key: &str, expected: Value) {
    let actual = object
        .remove(key)
        .unwrap_or_else(|| panic!("frozen contract lost {key}"));
    assert_eq!(actual, expected, "frozen contract changed {key}");
}

fn replace_exact(object: &mut Map<String, Value>, key: &str, expected: Value, replacement: Value) {
    remove_exact(object, key, expected);
    assert!(
        object.insert(key.to_owned(), replacement).is_none(),
        "{key} unexpectedly remained after exact removal"
    );
}

fn insert_delta(object: &mut Map<String, Value>, key: &str, value: Value) {
    assert!(
        !object.contains_key(key),
        "frozen contract unexpectedly already contains additive delta {key}"
    );
    object.insert(key.to_owned(), value);
}

fn replace_exact_once(text: &str, old: &str, new: &str) -> String {
    assert_eq!(
        text.matches(old).count(),
        1,
        "frozen contract changed approved text delta {old:?}"
    );
    text.replacen(old, new, 1)
}

fn normalize_protocol_metadata(result: &mut Map<String, Value>) {
    if let Some(result_type) = result.remove("resultType") {
        assert_eq!(result_type, "complete", "unexpected resultType shape");
    }
    if let Some(ttl_ms) = result.remove("ttlMs") {
        assert!(ttl_ms.as_u64().is_some(), "unexpected ttlMs shape");
    }
    if let Some(cache_scope) = result.remove("cacheScope") {
        assert!(
            matches!(cache_scope.as_str(), Some("public" | "private")),
            "unexpected cacheScope shape"
        );
    }
}

fn canonical_initialize(mut result: Value, expected: bool) -> Value {
    let object = result
        .as_object_mut()
        .expect("initialize result is an object");
    let server_info = object["serverInfo"]
        .as_object_mut()
        .expect("serverInfo is an object");
    assert!(
        server_info.get("version").is_some_and(Value::is_string),
        "implementation version must be a string"
    );
    server_info.insert("version".to_owned(), Value::String("<ignored>".to_owned()));
    if expected {
        replace_exact(
            object,
            "instructions",
            Value::String(V1_INITIALIZE_INSTRUCTIONS.to_owned()),
            Value::String(RUST_INITIALIZE_INSTRUCTIONS.to_owned()),
        );
    }
    result
}

fn structured_output_schema() -> Value {
    serde_json::json!({
        "additionalProperties": false,
        "properties": {
            "data": {},
            "meta": {
                "additionalProperties": false,
                "properties": {"source": {"type": "string"}},
                "required": ["source"],
                "type": "object"
            }
        },
        "required": ["data", "meta"],
        "type": "object"
    })
}

fn v1_output_schema() -> Value {
    serde_json::json!({
        "description": "Generic wrapper for non-object return types.",
        "properties": {"result": {"type": "string"}},
        "required": ["result"],
        "type": "object",
        "x-fastmcp-wrap-result": true
    })
}

fn canonical_tools(mut tools: Vec<Value>, expected: bool) -> Vec<Value> {
    for tool in &mut tools {
        if expected {
            if tool["name"] == "get_crypto_quote" {
                let old = tool["description"].as_str().unwrap();
                let description = replace_exact_once(
                    old,
                    "Get current top-of-book crypto prices.",
                    "Get current crypto prices.",
                );
                tool["description"] = Value::String(description);
            }
            replace_exact(
                tool.as_object_mut().unwrap(),
                "outputSchema",
                v1_output_schema(),
                structured_output_schema(),
            );
        }
        tool["inputSchema"] = normalize_schema(tool["inputSchema"].take());
        tool["outputSchema"] = normalize_schema(tool["outputSchema"].take());
    }
    sort_by_string_field(&mut tools, "name");
    tools
}

fn legacy_tools(tools: Vec<Value>, baseline: &Value) -> Vec<Value> {
    let legacy_names = baseline["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    let mut legacy_tools = tools
        .into_iter()
        .filter(|tool| legacy_names.contains(tool["name"].as_str().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        legacy_tools.len(),
        legacy_names.len(),
        "legacy tools are missing"
    );
    for tool in &mut legacy_tools {
        if matches!(
            tool["name"].as_str(),
            Some(
                "get_intraday_prices"
                    | "get_daily_fundamentals"
                    | "get_company_meta"
                    | "get_dividend_yield"
            )
        ) {
            let properties = tool["inputSchema"]["properties"]
                .as_object_mut()
                .expect("legacy tool input schema properties are an object");
            let columns = properties
                .remove("columns")
                .expect("legacy columns extension is present");
            assert_eq!(
                normalize_schema(columns),
                serde_json::json!({
                    "default": null,
                    "items": {"type": "string"},
                    "type": ["array", "null"]
                }),
                "legacy columns extension drifted"
            );
        }
    }
    legacy_tools
}

fn canonical_list(mut items: Vec<Value>, field: &str) -> Vec<Value> {
    sort_by_string_field(&mut items, field);
    items
}

fn canonical_resources(mut resources: Vec<Value>, expected: bool) -> Vec<Value> {
    if expected {
        for resource in &mut resources {
            replace_exact(
                resource.as_object_mut().unwrap(),
                "mimeType",
                Value::String("text/plain".to_owned()),
                Value::String("application/json".to_owned()),
            );
            if resource["uri"] == "tiingo://capabilities" {
                replace_exact(
                    resource.as_object_mut().unwrap(),
                    "description",
                    Value::String(
                        "Server capabilities, supported asset classes, rate limits, and plan restrictions"
                            .to_owned(),
                    ),
                    Value::String(
                        "Server capabilities and source-dated entitlement guidance".to_owned(),
                    ),
                );
            }
        }
    }
    canonical_list(resources, "uri")
}

fn canonical_resource_templates(mut templates: Vec<Value>, expected: bool) -> Vec<Value> {
    if expected {
        for template in &mut templates {
            replace_exact(
                template.as_object_mut().unwrap(),
                "mimeType",
                Value::String("text/plain".to_owned()),
                Value::String("application/json".to_owned()),
            );
        }
    }
    canonical_list(templates, "uriTemplate")
}

fn expected_resource_body(uri: &str, mut body: Value) -> Value {
    let object = body.as_object_mut().unwrap();
    match uri {
        "tiingo://capabilities" => {
            remove_exact(object, "server_version", serde_json::json!("1.1.0"));
            replace_exact(
                object,
                "tool_count",
                Value::Number(17.into()),
                Value::Number(38.into()),
            );
            remove_exact(
                object,
                "rate_limits",
                serde_json::json!({"free": "50 req/hr", "power": "5000 req/hr"}),
            );
            remove_exact(
                object,
                "plan_restrictions",
                serde_json::json!({
                    "free_tier": [
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
                        "get_dividend_yield"
                    ],
                    "paid_tier_required": ["get_dividends", "get_splits"]
                }),
            );
            insert_delta(object, "as_of", Value::String(SOURCE_DATE.to_owned()));
            insert_delta(object, "entitlements_change_over_time", Value::Bool(true));
            insert_delta(
                object,
                "official_sources",
                serde_json::json!(OFFICIAL_SOURCES),
            );
        }
        "tiingo://guide/corporate-actions"
        | "tiingo://guide/crypto"
        | "tiingo://guide/forex"
        | "tiingo://guide/fundamentals"
        | "tiingo://guide/news"
        | "tiingo://guide/stocks" => {
            let old_restriction = match uri {
                "tiingo://guide/corporate-actions" => {
                    "get_dividends and get_splits return 403 on free tier; get_dividend_yield is available on free tier."
                }
                "tiingo://guide/crypto" | "tiingo://guide/forex" | "tiingo://guide/news" => {
                    "Available on free tier."
                }
                "tiingo://guide/fundamentals" => {
                    "All fundamentals endpoints available on free tier."
                }
                "tiingo://guide/stocks" => "All stock endpoints available on free tier.",
                _ => unreachable!(),
            };
            remove_exact(
                object,
                "plan_restrictions",
                Value::String(old_restriction.to_owned()),
            );
            let official_sources = match uri {
                "tiingo://guide/crypto" => serde_json::json!([
                    OFFICIAL_SOURCES[0],
                    "https://www.tiingo.com/documentation/crypto"
                ]),
                "tiingo://guide/forex" => serde_json::json!([
                    OFFICIAL_SOURCES[0],
                    "https://www.tiingo.com/documentation/forex"
                ]),
                "tiingo://guide/fundamentals" => serde_json::json!([
                    OFFICIAL_SOURCES[0],
                    "https://www.tiingo.com/documentation/fundamentals"
                ]),
                "tiingo://guide/news" => serde_json::json!([
                    OFFICIAL_SOURCES[0],
                    "https://www.tiingo.com/documentation/news"
                ]),
                _ => serde_json::json!(OFFICIAL_SOURCES),
            };
            insert_delta(
                object,
                "availability",
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": official_sources,
                    "statement": "Access depends on current Tiingo account entitlements; a 403 means this credential is not entitled to the requested capability."
                }),
            );
            if uri == "tiingo://guide/corporate-actions" {
                let pitfalls = object["common_pitfalls"].as_array_mut().unwrap();
                assert_eq!(
                    pitfalls.first(),
                    Some(&serde_json::json!(
                        "get_dividends and get_splits require a paid plan -- free tier returns 403."
                    )),
                    "frozen corporate-actions entitlement pitfall changed"
                );
                pitfalls.remove(0);
            } else if uri == "tiingo://guide/crypto" {
                insert_delta(
                    object,
                    "current_price_route",
                    Value::String("/tiingo/crypto/prices".to_owned()),
                );
            } else if uri == "tiingo://guide/fundamentals" {
                let pitfalls = object["common_pitfalls"].as_array_mut().unwrap();
                assert_eq!(
                    pitfalls.first(),
                    Some(&serde_json::json!(
                        "Financial statements are reported quarterly; don't expect daily granularity."
                    )),
                    "frozen fundamentals cadence pitfall changed"
                );
                pitfalls[0] = serde_json::json!(
                    "Financial statements are reported quarterly and annually; don't expect daily granularity."
                );
            } else if uri == "tiingo://guide/stocks" {
                replace_exact(
                    object,
                    "ticker_format",
                    Value::String("Uppercase symbols, e.g. AAPL, MSFT, GOOGL, BRK.B".to_owned()),
                    Value::String("Uppercase symbols, e.g. AAPL, MSFT, GOOGL, BRK-A".to_owned()),
                );
            }
        }
        _ => {}
    }
    body
}

fn remove_task8_resource_delta(uri: &str, mut body: Value) -> Value {
    let object = body.as_object_mut().unwrap();
    let entitlement_statement = "Access depends on current Tiingo account entitlements; a 403 means this credential is not entitled to the requested capability.";
    match uri {
        "tiingo://capabilities" => {
            replace_exact(
                object,
                "as_of",
                Value::String("2026-08-25".to_owned()),
                Value::String(SOURCE_DATE.to_owned()),
            );
            replace_exact(
                object,
                "asset_classes",
                serde_json::json!({
                    "corporate_actions": {
                        "data_types": ["dividends", "distribution_yield", "splits", "batch_ex_date"],
                        "description": "Per-ticker and cross-ticker distributions and splits"
                    },
                    "crypto": {
                        "data_types": ["realtime_quote", "historical_prices", "metadata"],
                        "description": "Cryptocurrencies across exchanges"
                    },
                    "crypto_yield": {
                        "data_types": ["platforms", "pools", "latest_ticks", "historical_metrics"],
                        "description": "Crypto lending platform and pool metrics"
                    },
                    "forex": {
                        "data_types": ["realtime_quote", "batch_realtime_quotes", "historical_prices"],
                        "description": "Foreign exchange currency pairs"
                    },
                    "funds": {
                        "data_types": ["metadata", "fee_metrics"],
                        "description": "Mutual-fund and ETF fee data"
                    },
                    "news": {
                        "data_types": ["article_search"],
                        "description": "Filterable financial news articles"
                    },
                    "stocks": {
                        "data_types": ["eod_prices", "bulk_eod_refresh", "lifecycle_metadata", "iex_realtime", "iex_intraday", "consolidated_realtime", "boats_overnight", "fundamentals"],
                        "description": "Equity EOD, intraday, realtime, lifecycle, and company data"
                    }
                }),
                serde_json::json!({
                    "crypto": {
                        "data_types": ["realtime_quote", "historical_prices", "metadata"],
                        "description": "Cryptocurrencies across exchanges"
                    },
                    "forex": {
                        "data_types": ["realtime_quote", "historical_prices"],
                        "description": "Foreign exchange currency pairs"
                    },
                    "stocks": {
                        "data_types": ["eod_prices", "intraday_prices", "realtime_quote", "metadata"],
                        "description": "US and international equities"
                    }
                }),
            );
            replace_exact(
                object,
                "official_sources",
                serde_json::json!([
                    "https://www.tiingo.com/documentation/general/overview",
                    "https://www.tiingo.com/documentation/end-of-day",
                    "https://www.tiingo.com/kb/article/the-fastest-method-to-ingest-tiingo-end-of-day-stock-api-data/",
                    "https://www.tiingo.com/documentation/iex",
                    "https://www.tiingo.com/documentation/equity-realtime-stock-data",
                    "https://www.tiingo.com/documentation/boats",
                    "https://www.tiingo.com/documentation/forex",
                    "https://www.tiingo.com/documentation/crypto",
                    "https://www.tiingo.com/documentation/crypto-yield",
                    "https://www.tiingo.com/documentation/news",
                    "https://www.tiingo.com/documentation/fundamentals",
                    "https://www.tiingo.com/documentation/mutual-fund-and-etf-fees",
                    "https://www.tiingo.com/documentation/corporate-actions/dividends",
                    "https://www.tiingo.com/documentation/corporate-actions/splits",
                    "https://www.tiingo.com/documentation/utilities/search",
                    "https://www.tiingo.com/documentation/websockets/iex",
                    "https://www.tiingo.com/documentation/websockets/equity-realtime-stock-data"
                ]),
                serde_json::json!(OFFICIAL_SOURCES),
            );
            remove_exact(
                object,
                "access",
                serde_json::json!({
                    "authentication": "HTTP 401 means Tiingo rejected the credential.",
                    "authorization": "HTTP 403 or an upstream WebSocket authorization rejection means the credential is not entitled to the requested capability.",
                    "bulk_usage": "Every live call consumes quota or bandwidth; bulk and all-market operations are not routine live smoke tests.",
                    "crypto_yield": "Plan and entitlement dependent.",
                    "fund_fees": "Enterprise or institutional access.",
                    "fundamentals_and_corporate_actions": "Entitlement dependent.",
                    "search": "Early beta; response fields can change.",
                    "ticker_lifecycle_metadata": "Vendor-supplied and availability dependent."
                }),
            );
            remove_exact(
                object,
                "upstream_market_data",
                serde_json::json!({
                    "description": "Finite start, poll, update, and stop lifecycle over upstream Tiingo WebSockets; MCP transport remains stdio.",
                    "services": {
                        "consolidated": {
                            "hours": "4am-8pm ET",
                            "status": "beta",
                            "threshold_levels": [4, 6]
                        },
                        "iex": {
                            "agreement_confirmation_levels": [0, 5],
                            "default_threshold_level": 6
                        }
                    }
                }),
            );
            remove_exact(
                object,
                "utilities",
                serde_json::json!({
                    "search": {
                        "data_types": ["asset_search"],
                        "status": "early beta"
                    }
                }),
            );
        }
        "tiingo://guide/corporate-actions" => {
            replace_exact(
                object,
                "availability",
                serde_json::json!({
                    "as_of": "2026-08-25",
                    "official_sources": [
                        "https://www.tiingo.com/documentation/general/overview",
                        "https://www.tiingo.com/documentation/corporate-actions/dividends",
                        "https://www.tiingo.com/documentation/corporate-actions/splits"
                    ],
                    "statement": entitlement_statement
                }),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": OFFICIAL_SOURCES,
                    "statement": entitlement_statement
                }),
            );
            replace_exact(
                object,
                "common_pitfalls",
                serde_json::json!([
                    "Date params for dividends/splits filter by ex-date, not payment date.",
                    "Batch ex-date results can include announced future actions, and future splits can be cancelled.",
                    "ETFs (e.g. SPY, QQQ) have dividend data via get_dividends."
                ]),
                serde_json::json!([
                    "Date params for dividends/splits filter by ex-date, not payment date.",
                    "ETFs (e.g. SPY, QQQ) have dividend data via get_dividends."
                ]),
            );
            replace_exact(
                object,
                "tools",
                serde_json::json!([
                    "get_distributions_by_ex_date",
                    "get_dividends",
                    "get_dividend_yield",
                    "get_splits",
                    "get_splits_by_ex_date"
                ]),
                serde_json::json!(["get_dividends", "get_dividend_yield", "get_splits"]),
            );
            replace_exact(
                object,
                "workflows",
                serde_json::json!([
                    "Use get_dividends for historical cash distributions; dates filter by ex-dividend date.",
                    "Use get_dividend_yield for time-series of yield percentage.",
                    "Use get_splits for historical split events; dates filter by ex-split date.",
                    "Use get_distributions_by_ex_date or get_splits_by_ex_date for cross-ticker exact-date queries and future announcements.",
                    "For dividends and splits, start_date/end_date map to startExDate/endExDate internally.",
                    "For dividend_yield, start_date/end_date map to startDate/endDate."
                ]),
                serde_json::json!([
                    "Use get_dividends for historical cash distributions; dates filter by ex-dividend date.",
                    "Use get_dividend_yield for time-series of yield percentage.",
                    "Use get_splits for historical split events; dates filter by ex-split date.",
                    "For dividends and splits, start_date/end_date map to startExDate/endExDate internally.",
                    "For dividend_yield, start_date/end_date map to startDate/endDate."
                ]),
            );
        }
        "tiingo://guide/crypto" => {
            replace_exact(
                object,
                "availability",
                serde_json::json!({
                    "as_of": "2026-08-25",
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/crypto"],
                    "statement": entitlement_statement
                }),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/crypto"],
                    "statement": entitlement_statement
                }),
            );
            replace_exact(
                object,
                "common_pitfalls",
                serde_json::json!([
                    "Crypto tickers are lowercase (btcusd, not BTCUSD).",
                    "Crypto trades 24/7 -- no market-hours limitation.",
                    "get_crypto_quote with no tickers returns a large payload; filter by tickers when possible.",
                    "The deprecated /tiingo/crypto/top route is not implemented; current quotes use /tiingo/crypto/prices."
                ]),
                serde_json::json!([
                    "Crypto tickers are lowercase (btcusd, not BTCUSD).",
                    "Crypto trades 24/7 -- no market-hours limitation.",
                    "get_crypto_quote with no tickers returns a large payload; filter by tickers when possible."
                ]),
            );
            replace_exact(
                object,
                "workflows",
                serde_json::json!([
                    "Use get_crypto_metadata to discover available tickers and supported exchanges.",
                    "Use get_crypto_quote for current prices; omit tickers to get all supported cryptos.",
                    "Use get_crypto_prices for historical OHLCV data at various intraday or daily intervals.",
                    "Pass comma-separated tickers to get_crypto_prices for multiple assets at once.",
                    "Use the separate crypto-yield guide for lending platforms, pools, ticks, and metrics."
                ]),
                serde_json::json!([
                    "Use get_crypto_metadata to discover available tickers and supported exchanges.",
                    "Use get_crypto_quote for current prices; omit tickers to get all supported cryptos.",
                    "Use get_crypto_prices for historical OHLCV data at various intraday or daily intervals.",
                    "Pass comma-separated tickers to get_crypto_prices for multiple assets at once."
                ]),
            );
        }
        "tiingo://guide/forex" => {
            replace_exact(
                object,
                "availability",
                serde_json::json!({
                    "as_of": "2026-08-25",
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/forex"],
                    "statement": entitlement_statement
                }),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/forex"],
                    "statement": entitlement_statement
                }),
            );
            replace_exact(
                object,
                "tools",
                serde_json::json!(["get_forex_quote", "get_forex_quotes", "get_forex_prices"]),
                serde_json::json!(["get_forex_quote", "get_forex_prices"]),
            );
            replace_exact(
                object,
                "workflows",
                serde_json::json!([
                    "Use get_forex_quote for the current top-of-book bid/ask for a pair.",
                    "Use get_forex_quotes for a bounded batch of one to 100 currency pairs.",
                    "Use get_forex_prices for historical OHLCV data; supports 1min to 1day resampling."
                ]),
                serde_json::json!([
                    "Use get_forex_quote for the current top-of-book bid/ask for a pair.",
                    "Use get_forex_prices for historical OHLCV data; supports 1min to 1day resampling."
                ]),
            );
        }
        "tiingo://guide/fundamentals" => {
            replace_exact(
                object,
                "availability",
                serde_json::json!({
                    "as_of": "2026-08-25",
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/fundamentals"],
                    "statement": entitlement_statement
                }),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/fundamentals"],
                    "statement": entitlement_statement
                }),
            );
            replace_exact(
                object,
                "common_pitfalls",
                serde_json::json!([
                    "Financial statements are reported quarterly and annually; don't expect daily granularity.",
                    "get_daily_fundamentals returns many rows -- use dates and columns to limit results.",
                    "get_company_meta accepts comma-separated tickers and optional columns for bounded batch lookups."
                ]),
                serde_json::json!([
                    "Financial statements are reported quarterly and annually; don't expect daily granularity.",
                    "get_daily_fundamentals returns many rows -- use start_date/end_date to limit results.",
                    "get_company_meta accepts comma-separated tickers for batch lookups."
                ]),
            );
            replace_exact(
                object,
                "workflows",
                serde_json::json!([
                    "Call get_fundamentals_definitions once to understand available metrics and their types.",
                    "Use get_financial_statements for quarterly/annual income, balance sheet, and cash flow data.",
                    "Use get_daily_fundamentals for time-series of daily metrics like marketCap and P/E ratio; select columns when the full response is unnecessary.",
                    "Use get_company_meta for sector, industry, country, and SIC code; it supports multiple tickers and selected columns."
                ]),
                serde_json::json!([
                    "Call get_fundamentals_definitions once to understand available metrics and their types.",
                    "Use get_financial_statements for quarterly/annual income, balance sheet, and cash flow data.",
                    "Use get_daily_fundamentals for time-series of daily metrics like marketCap and P/E ratio.",
                    "Use get_company_meta for sector, industry, country, and SIC code; supports multiple tickers."
                ]),
            );
        }
        "tiingo://guide/news" => {
            replace_exact(
                object,
                "availability",
                serde_json::json!({
                    "as_of": "2026-08-25",
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/news"],
                    "statement": entitlement_statement
                }),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": [OFFICIAL_SOURCES[0], "https://www.tiingo.com/documentation/news"],
                    "statement": entitlement_statement
                }),
            );
            replace_exact(
                object,
                "common_pitfalls",
                serde_json::json!([
                    "Default limit is 10 articles -- increase for broader searches.",
                    "Omitting tickers returns broad market news, not company-specific.",
                    "sort_by values are crawlDate and publishedDate (exact case required).",
                    "Institutional bulk downloads are excluded because their download URLs contain credentials and their payloads are unbounded."
                ]),
                serde_json::json!([
                    "Default limit is 10 articles -- increase for broader searches.",
                    "Omitting tickers returns broad market news, not company-specific.",
                    "sort_by values are crawlDate and publishedDate (exact case required)."
                ]),
            );
            replace_exact(
                object,
                "workflows",
                serde_json::json!([
                    "Filter by tickers to get company-specific news; multiple tickers = OR logic.",
                    "Filter by tags for topic-based news (e.g. earnings, dividends).",
                    "Use start_date/end_date to restrict date range; default returns most recent.",
                    "Paginate with limit and offset for large result sets.",
                    "Use sort_by='crawlDate' for recency or 'publishedDate' for article publish date.",
                    "Use the separate search guide to find Tiingo assets by ticker or name; it does not search articles."
                ]),
                serde_json::json!([
                    "Filter by tickers to get company-specific news; multiple tickers = OR logic.",
                    "Filter by tags for topic-based news (e.g. earnings, dividends).",
                    "Use start_date/end_date to restrict date range; default returns most recent.",
                    "Paginate with limit and offset for large result sets.",
                    "Use sort_by='crawlDate' for recency or 'publishedDate' for article publish date."
                ]),
            );
        }
        "tiingo://guide/stocks" => {
            replace_exact(
                object,
                "availability",
                serde_json::json!({
                    "as_of": "2026-08-25",
                    "official_sources": [
                        "https://www.tiingo.com/documentation/general/overview",
                        "https://www.tiingo.com/documentation/end-of-day",
                        "https://www.tiingo.com/kb/article/the-fastest-method-to-ingest-tiingo-end-of-day-stock-api-data/",
                        "https://www.tiingo.com/documentation/iex",
                        "https://www.tiingo.com/documentation/equity-realtime-stock-data",
                        "https://www.tiingo.com/documentation/boats"
                    ],
                    "statement": entitlement_statement
                }),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": OFFICIAL_SOURCES,
                    "statement": entitlement_statement
                }),
            );
            replace_exact(
                object,
                "common_pitfalls",
                serde_json::json!([
                    "Ticker symbols are case-sensitive in the URL -- always uppercase.",
                    "Intraday data is limited to recent history; check metadata for available date range.",
                    "get_realtime_price may return stale data outside market hours unless after_hours=True.",
                    "The vendor-supplied /tiingo/daily/meta route is availability-dependent and requires an explicit columns list.",
                    "Consolidated 4am-8pm ET and BOATS 8pm-3:59am ET are separate beta products, not one 24x5 endpoint."
                ]),
                serde_json::json!([
                    "Ticker symbols are case-sensitive in the URL -- always uppercase.",
                    "Intraday data is limited to recent history; check metadata for available date range.",
                    "get_realtime_price may return stale data outside market hours unless after_hours=True."
                ]),
            );
            replace_exact(
                object,
                "description",
                Value::String("Equities with EOD and bulk refresh, lifecycle metadata, IEX, consolidated realtime, and BOATS overnight data.".to_owned()),
                Value::String("US and international equities with EOD historical prices, real-time IEX quotes, and intraday data.".to_owned()),
            );
            replace_exact(
                object,
                "tools",
                serde_json::json!([
                    "get_stock_metadata",
                    "get_stock_prices",
                    "get_bulk_eod_prices",
                    "get_ticker_metadata",
                    "get_realtime_price",
                    "get_intraday_prices",
                    "get_iex_market_snapshot",
                    "get_equity_realtime_snapshot",
                    "get_equity_intraday_prices",
                    "get_boats_snapshot",
                    "get_boats_prices"
                ]),
                serde_json::json!([
                    "get_stock_metadata",
                    "get_stock_prices",
                    "get_realtime_price",
                    "get_intraday_prices"
                ]),
            );
            replace_exact(
                object,
                "workflows",
                serde_json::json!([
                    "Fetch metadata first with get_stock_metadata to verify ticker validity and date range.",
                    "Use get_stock_prices for EOD OHLCV history; supports daily/weekly/monthly/annually resampling.",
                    "Use get_bulk_eod_prices for daily cache refresh, then reseed ticker history when splitFactor != 1 or divCash > 0.",
                    "Use get_ticker_metadata only with the lifecycle columns needed for the task.",
                    "Use get_realtime_price for current IEX top-of-book price during market hours.",
                    "Use get_intraday_prices for sub-daily IEX data at 1min, 5min, 15min, 30min, or 1hour intervals.",
                    "Use ticker-filtered consolidated or BOATS tools before considering their large all-market snapshots."
                ]),
                serde_json::json!([
                    "Fetch metadata first with get_stock_metadata to verify ticker validity and date range.",
                    "Use get_stock_prices for EOD OHLCV history; supports daily/weekly/monthly/annually resampling.",
                    "Use get_realtime_price for current IEX top-of-book price during market hours.",
                    "Use get_intraday_prices for sub-daily data at 1min, 5min, 15min, 30min, or 1hour intervals."
                ]),
            );
        }
        _ => {}
    }
    body
}

fn canonical_resource_result(uri: &str, mut result: Value, expected: bool) -> Value {
    normalize_protocol_metadata(result.as_object_mut().unwrap());
    for content in result["contents"].as_array_mut().unwrap() {
        if expected {
            replace_exact(
                content.as_object_mut().unwrap(),
                "mimeType",
                Value::String("text/plain".to_owned()),
                Value::String("application/json".to_owned()),
            );
        }
        let mut body: Value = serde_json::from_str(content["text"].as_str().unwrap()).unwrap();
        if expected {
            body = expected_resource_body(uri, body);
        } else {
            body = remove_task8_resource_delta(uri, body);
            if uri == "tiingo://capabilities" {
                let version = body
                    .as_object_mut()
                    .unwrap()
                    .remove("server_version")
                    .expect("capabilities server_version is present");
                assert!(
                    version.is_string(),
                    "capabilities server_version is a string"
                );
            }
        }
        content["text"] = body;
    }
    result
}

fn corrected_prompt_text(name: &str, text: &str) -> String {
    match name {
        "analyze-stock" => replace_exact_once(
            &replace_exact_once(
                &replace_exact_once(
                    &replace_exact_once(
                        &replace_exact_once(
                            text,
                            "2. Call get_stock_prices",
                            "2. Call get_company_meta with tickers=AAPL to retrieve sector and industry.\n3. Call get_stock_prices",
                        ),
                        "identify key catalysts, analyst commentary, and market-moving events",
                        "identify reported catalysts and market-moving events",
                    ),
                    "- **Recent Catalysts**: Key news stories or events driving price movement (if news was fetched).",
                    "- **Recent Catalysts**: Reported news or events if news was fetched; do not infer causes absent evidence.",
                ),
                "\n3. Call get_daily_fundamentals",
                "\n4. Call get_daily_fundamentals",
            ),
            "\n4. Call get_news",
            "\n5. Call get_news",
        ),
        "earnings-report-analysis" => replace_exact_once(
            text,
            "- **Beat or Miss**: Did the company beat or miss expectations based on trends?",
            "- **Expectations Context**: Do not label the result a beat or miss unless an article supplies an explicit consensus comparison.",
        ),
        "forex-pair-analysis" => replace_exact_once(
            text,
            "- **Notable Moves**: Any significant spikes or drops and their likely causes.",
            "- **Notable Moves**: Identify significant spikes or drops, but state that price history alone cannot establish their cause.",
        ),
        _ => text.to_owned(),
    }
}

fn canonical_prompt_result(name: &str, mut result: Value, expected: bool) -> Value {
    normalize_protocol_metadata(result.as_object_mut().unwrap());
    if expected {
        let text = result["messages"][0]["content"]["text"].as_str().unwrap();
        result["messages"][0]["content"]["text"] = Value::String(corrected_prompt_text(name, text));
    }
    result
}

fn prompt_requests() -> [(String, Map<String, Value>); 5] {
    [
        (
            "analyze-stock".to_owned(),
            arguments(serde_json::json!({"ticker": "AAPL"})),
        ),
        (
            "compare-stocks".to_owned(),
            arguments(serde_json::json!({"ticker1": "AAPL", "ticker2": "MSFT"})),
        ),
        ("crypto-market-overview".to_owned(), JsonObject::new()),
        (
            "earnings-report-analysis".to_owned(),
            arguments(serde_json::json!({
                "ticker": "NVDA",
                "earnings_date": "2024-02-21"
            })),
        ),
        (
            "forex-pair-analysis".to_owned(),
            arguments(serde_json::json!({"pair": "eurusd"})),
        ),
    ]
}

async fn child_process_contract() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(V1_CONTRACT)?;
    let client = ().serve(TokioChildProcess::new(child_command())?).await?;

    assert_eq!(client.list_all_tools().await?.len(), 38);
    assert_eq!(client.list_all_resources().await?.len(), 3);
    assert_eq!(client.list_all_resource_templates().await?.len(), 1);
    assert_eq!(client.list_all_prompts().await?.len(), 5);

    let expected_initialize = canonical_initialize(baseline["initialize"].clone(), true);
    let actual_initialize =
        canonical_initialize(serde_json::to_value(client.peer_info().unwrap())?, false);
    assert_eq!(actual_initialize, expected_initialize, "initialize drifted");

    let actual_tools = serde_json::to_value(client.list_all_tools().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_tools(legacy_tools(actual_tools, &baseline), false),
        canonical_tools(baseline["tools"].as_array().unwrap().clone(), true),
        "tool discovery drifted"
    );

    let actual_resources = serde_json::to_value(client.list_all_resources().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_resources(actual_resources, false),
        canonical_resources(baseline["resources"].as_array().unwrap().clone(), true),
        "resource discovery drifted"
    );
    let actual_templates = serde_json::to_value(client.list_all_resource_templates().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_resource_templates(actual_templates, false),
        canonical_resource_templates(
            baseline["resource_templates"].as_array().unwrap().clone(),
            true
        ),
        "resource-template discovery drifted"
    );
    let actual_prompts = serde_json::to_value(client.list_all_prompts().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_list(actual_prompts, "name"),
        canonical_list(baseline["prompts"].as_array().unwrap().clone(), "name"),
        "prompt discovery drifted"
    );

    for (uri, expected) in baseline["resource_contents"].as_object().unwrap() {
        let actual = serde_json::to_value(
            client
                .read_resource(ReadResourceRequestParams::new(uri))
                .await?,
        )?;
        assert_eq!(
            canonical_resource_result(uri, actual, false),
            canonical_resource_result(uri, serde_json::json!({"contents": expected.clone()}), true),
            "{uri} content drifted"
        );
    }

    for (name, request_arguments) in prompt_requests() {
        let actual = serde_json::to_value(
            client
                .get_prompt(GetPromptRequestParams::new(&name).with_arguments(request_arguments))
                .await?,
        )?;
        assert_eq!(
            canonical_prompt_result(&name, actual, false),
            canonical_prompt_result(&name, baseline["prompt_results"][&name].clone(), true),
            "{name} result drifted"
        );
    }

    let missing_key = client
        .call_tool(
            CallToolRequestParams::new("get_stock_metadata")
                .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"}))),
        )
        .await?;
    assert_eq!(missing_key.is_error, Some(true));
    assert_eq!(
        missing_key.structured_content.as_ref().unwrap()["error"]["kind"],
        "configuration"
    );
    let text = serde_json::to_value(&missing_key.content[0])?;
    let payload: Value = serde_json::from_str(text["text"].as_str().unwrap())?;
    assert_eq!(payload["kind"], "configuration");
    assert!(!payload.to_string().contains("TIINGO_API_KEY="));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn child_process_matches_the_frozen_contract_plus_exact_approved_deltas() -> anyhow::Result<()>
{
    tokio::time::timeout(CHILD_TIMEOUT, child_process_contract())
        .await
        .context("compiled child contract timed out")??;
    Ok(())
}

#[test]
fn frozen_v1_rejected_current_discover_with_the_exact_legacy_error() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(V1_CONTRACT)?;
    assert_eq!(
        baseline["server_discover"],
        serde_json::json!({
            "error": {
                "code": -32602,
                "data": "",
                "message": "Invalid request parameters"
            }
        })
    );
    Ok(())
}

#[tokio::test]
async fn approved_crypto_route_and_bounded_retry_delta_are_explicit() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(V1_CONTRACT)?;
    assert!(
        baseline["http_requests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|request| request["path"] == "/tiingo/crypto/top")
    );

    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/crypto/prices"))
        .and(query_param("tickers", "btcusd"))
        .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
        .up_to_n_times(2)
        .mount(&upstream)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/crypto/prices"))
        .and(query_param("tickers", "btcusd"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&upstream)
        .await;

    let client = TiingoClient::new(Config {
        api_key: Some("contract-key".to_owned()),
        base_url: Url::parse(&upstream.uri())?,
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: MAX_RESPONSE_BYTES,
    })?;
    assert_eq!(
        client.get_crypto_quote(Some("btcusd")).await?,
        serde_json::json!({"ok": true})
    );
    Ok(())
}

#[test]
fn canonicalization_preserves_unapproved_metadata() {
    let items = vec![serde_json::json!({
        "name": "example",
        "_meta": {"future-extension": {"enabled": true}}
    })];

    assert_eq!(canonical_list(items.clone(), "name"), items);
}

#[test]
fn approved_resource_discovery_delta_rejects_mutated_frozen_source() {
    let baseline: Value = serde_json::from_str(V1_CONTRACT).unwrap();
    let mut resources = baseline["resources"].as_array().unwrap().clone();
    resources[0]["mimeType"] = Value::String("application/json".to_owned());
    assert_transform_rejects("resource MIME type", move || {
        canonical_resources(resources, true);
    });

    let mut resources = baseline["resources"].as_array().unwrap().clone();
    resources[0]["description"] =
        Value::String("Server capabilities and source-dated entitlement guidance".to_owned());
    assert_transform_rejects("capabilities description", move || {
        canonical_resources(resources, true);
    });

    let mut templates = baseline["resource_templates"].as_array().unwrap().clone();
    templates[0]["mimeType"] = Value::String("application/json".to_owned());
    assert_transform_rejects("resource template MIME type", move || {
        canonical_resource_templates(templates, true);
    });

    let mut contents = serde_json::json!({
        "contents": baseline["resource_contents"]["tiingo://capabilities"].clone()
    });
    contents["contents"][0]["mimeType"] = Value::String("application/json".to_owned());
    assert_transform_rejects("resource content MIME type", move || {
        canonical_resource_result("tiingo://capabilities", contents, true);
    });
}

#[test]
fn approved_output_schema_delta_rejects_a_mutated_frozen_source() {
    let baseline: Value = serde_json::from_str(V1_CONTRACT).unwrap();
    let mut tools = baseline["tools"].as_array().unwrap().clone();
    tools[0]["outputSchema"]["properties"]["result"]["type"] = Value::String("number".into());

    assert!(
        std::panic::catch_unwind(|| canonical_tools(tools, true)).is_err(),
        "canonicalization overwrote a mutated frozen output schema"
    );
}

fn frozen_resource_body(baseline: &Value, uri: &str) -> Value {
    serde_json::from_str(
        baseline["resource_contents"][uri][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

fn assert_transform_rejects(label: &str, transform: impl FnOnce() + std::panic::UnwindSafe) {
    assert!(
        std::panic::catch_unwind(transform).is_err(),
        "canonicalization accepted mutated frozen {label}"
    );
}

#[test]
fn every_approved_delta_rejects_mutated_frozen_values() {
    let baseline: Value = serde_json::from_str(V1_CONTRACT).unwrap();

    let mut initialize = baseline["initialize"].clone();
    initialize["instructions"] = Value::String(RUST_INITIALIZE_INSTRUCTIONS.to_owned());
    assert_transform_rejects("initialize instructions", move || {
        canonical_initialize(initialize, true);
    });

    let mut tools = baseline["tools"].as_array().unwrap().clone();
    let crypto = tools
        .iter_mut()
        .find(|tool| tool["name"] == "get_crypto_quote")
        .unwrap();
    crypto["description"] = Value::String(
        crypto["description"]
            .as_str()
            .unwrap()
            .replace("top-of-book crypto", "crypto"),
    );
    assert_transform_rejects("crypto description", move || {
        canonical_tools(tools, true);
    });

    for key in [
        "server_version",
        "tool_count",
        "rate_limits",
        "plan_restrictions",
        "as_of",
        "entitlements_change_over_time",
        "official_sources",
    ] {
        let mut body = frozen_resource_body(&baseline, "tiingo://capabilities");
        body[key] = Value::String("mutated".to_owned());
        assert_transform_rejects(key, move || {
            expected_resource_body("tiingo://capabilities", body);
        });
    }

    for uri in [
        "tiingo://guide/corporate-actions",
        "tiingo://guide/crypto",
        "tiingo://guide/forex",
        "tiingo://guide/fundamentals",
        "tiingo://guide/news",
        "tiingo://guide/stocks",
    ] {
        let mut body = frozen_resource_body(&baseline, uri);
        body["plan_restrictions"] = Value::String("mutated".to_owned());
        assert_transform_rejects(uri, move || {
            expected_resource_body(uri, body);
        });
    }

    let mut corporate = frozen_resource_body(&baseline, "tiingo://guide/corporate-actions");
    corporate["common_pitfalls"][0] = Value::String("mutated".to_owned());
    assert_transform_rejects("corporate-actions pitfall", move || {
        expected_resource_body("tiingo://guide/corporate-actions", corporate);
    });

    let mut crypto = frozen_resource_body(&baseline, "tiingo://guide/crypto");
    crypto["current_price_route"] = Value::String("/mutated".to_owned());
    assert_transform_rejects("crypto current route", move || {
        expected_resource_body("tiingo://guide/crypto", crypto);
    });

    let mut fundamentals = frozen_resource_body(&baseline, "tiingo://guide/fundamentals");
    fundamentals["common_pitfalls"][0] = Value::String("mutated".to_owned());
    assert_transform_rejects("fundamentals cadence", move || {
        expected_resource_body("tiingo://guide/fundamentals", fundamentals);
    });

    let mut stocks = frozen_resource_body(&baseline, "tiingo://guide/stocks");
    stocks["ticker_format"] = Value::String("mutated".to_owned());
    assert_transform_rejects("stock symbology", move || {
        expected_resource_body("tiingo://guide/stocks", stocks);
    });

    let mut guide = frozen_resource_body(&baseline, "tiingo://guide/news");
    guide["availability"] = serde_json::json!({"mutated": true});
    assert_transform_rejects("availability source metadata", move || {
        expected_resource_body("tiingo://guide/news", guide);
    });

    for (name, old, replacement) in [
        (
            "analyze-stock",
            "identify key catalysts, analyst commentary, and market-moving events",
            "identify mutated catalysts",
        ),
        (
            "earnings-report-analysis",
            "- **Beat or Miss**: Did the company beat or miss expectations based on trends?",
            "- **Mutated**",
        ),
        (
            "forex-pair-analysis",
            "- **Notable Moves**: Any significant spikes or drops and their likely causes.",
            "- **Mutated**",
        ),
    ] {
        let text = baseline["prompt_results"][name]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .replace(old, replacement);
        assert_transform_rejects(name, move || {
            corrected_prompt_text(name, &text);
        });
    }

    let text = baseline["prompt_results"]["analyze-stock"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap()
        .replace(
            "- **Recent Catalysts**: Key news stories or events driving price movement (if news was fetched).",
            "- **Recent Catalysts**: Mutated.",
        );
    assert_transform_rejects("analyze-stock evidence qualification", move || {
        corrected_prompt_text("analyze-stock", &text);
    });
}
