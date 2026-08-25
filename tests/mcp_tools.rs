use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use anyhow::Context;
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, JsonObject},
    service::RunningService,
};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    mcp::{TiingoServer, tools::IntradayPricesArgs},
};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const TOOL_NAMES: [&str; 17] = [
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
];

#[test]
fn intraday_prices_rejects_daily_resample() {
    let error = serde_json::from_value::<IntradayPricesArgs>(serde_json::json!({
        "ticker": "AAPL",
        "resample_freq": "1day"
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

fn test_client(server: &MockServer, api_key: &str) -> TiingoClient {
    TiingoClient::new(Config {
        api_key: Some(api_key.to_owned()),
        base_url: Url::parse(&server.uri()).unwrap(),
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    })
    .unwrap()
}

struct Connection {
    client: RunningService<RoleClient, ()>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Connection {
    async fn new(tiingo_client: TiingoClient) -> Self {
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let server = tokio::spawn(async move {
            TiingoServer::with_client(tiingo_client)
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await.unwrap();
        Self { client, server }
    }

    async fn close(mut self) {
        self.client.close().await.unwrap();
        self.server.await.unwrap().unwrap();
    }
}

fn arguments(value: serde_json::Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

fn assert_accurate_success(
    tool_name: &str,
    result: &rmcp::model::CallToolResult,
    expected: &serde_json::Value,
) {
    assert_eq!(
        result.is_error,
        Some(false),
        "{tool_name} returned an error"
    );
    let text = &result.content[0]
        .as_text()
        .expect("successful tools retain a JSON text block")
        .text;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).unwrap(),
        *expected,
        "{tool_name} changed the upstream JSON text payload"
    );
    assert_eq!(
        result.structured_content,
        Some(serde_json::json!({
            "data": expected,
            "meta": {"source": "tiingo"}
        })),
        "{tool_name} changed the structured payload"
    );
}

struct ToolCase {
    name: &'static str,
    route: &'static str,
    arguments: serde_json::Value,
    response: serde_json::Value,
    query: &'static [(&'static str, &'static str)],
}

fn all_tool_cases() -> Vec<ToolCase> {
    vec![
        ToolCase {
            name: "get_stock_metadata",
            route: "/tiingo/daily/AAPL",
            arguments: serde_json::json!({"ticker": "AAPL"}),
            response: serde_json::json!({"ticker": "AAPL", "name": "Apple Inc."}),
            query: &[],
        },
        ToolCase {
            name: "get_stock_prices",
            route: "/tiingo/daily/AAPL/prices",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "weekly"
            }),
            response: serde_json::json!([{"date": "2024-01-05", "close": 185.92}]),
            query: &[
                ("endDate", "2024-01-31"),
                ("resampleFreq", "weekly"),
                ("startDate", "2024-01-01"),
            ],
        },
        ToolCase {
            name: "get_realtime_price",
            route: "/iex/AAPL",
            arguments: serde_json::json!({"ticker": "AAPL", "after_hours": true}),
            response: serde_json::json!([{"ticker": "AAPL", "tngoLast": 227.16}]),
            query: &[("afterHours", "true")],
        },
        ToolCase {
            name: "get_intraday_prices",
            route: "/iex/AAPL/prices",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "5min"
            }),
            response: serde_json::json!([{"date": "2024-01-02T14:30:00Z", "close": 185.12}]),
            query: &[
                ("endDate", "2024-01-31"),
                ("resampleFreq", "5min"),
                ("startDate", "2024-01-01"),
            ],
        },
        ToolCase {
            name: "get_forex_quote",
            route: "/tiingo/fx/eurusd/top",
            arguments: serde_json::json!({"ticker": "eurusd"}),
            response: serde_json::json!([{"ticker": "eurusd", "midPrice": 1.0812}]),
            query: &[],
        },
        ToolCase {
            name: "get_forex_prices",
            route: "/tiingo/fx/eurusd/prices",
            arguments: serde_json::json!({
                "ticker": "eurusd",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "1day"
            }),
            response: serde_json::json!([{"date": "2024-01-02", "close": 1.0942}]),
            query: &[
                ("endDate", "2024-01-31"),
                ("resampleFreq", "1day"),
                ("startDate", "2024-01-01"),
            ],
        },
        ToolCase {
            name: "get_crypto_quote",
            route: "/tiingo/crypto/prices",
            arguments: serde_json::json!({"tickers": "btcusd"}),
            response: serde_json::json!([{"ticker": "btcusd", "priceData": [{"close": 64000.25}]}]),
            query: &[("tickers", "btcusd")],
        },
        ToolCase {
            name: "get_crypto_prices",
            route: "/tiingo/crypto/prices",
            arguments: serde_json::json!({
                "tickers": "btcusd,ethusd",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "1hour"
            }),
            response: serde_json::json!([{"ticker": "btcusd", "priceData": [{"close": 64000.25}]}]),
            query: &[
                ("endDate", "2024-01-31"),
                ("resampleFreq", "1hour"),
                ("startDate", "2024-01-01"),
                ("tickers", "btcusd,ethusd"),
            ],
        },
        ToolCase {
            name: "get_crypto_metadata",
            route: "/tiingo/crypto",
            arguments: serde_json::json!({"tickers": "btcusd"}),
            response: serde_json::json!([{"ticker": "btcusd", "baseCurrency": "btc"}]),
            query: &[("tickers", "btcusd")],
        },
        ToolCase {
            name: "get_news",
            route: "/tiingo/news",
            arguments: serde_json::json!({
                "tickers": "AAPL",
                "tags": "earnings",
                "source": "reuters",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "limit": 2,
                "offset": 1,
                "sort_by": "publishedDate"
            }),
            response: serde_json::json!([{
                "id": 42,
                "title": "Apple reports results",
                "publishedDate": "2024-01-25T21:00:00Z"
            }]),
            query: &[
                ("endDate", "2024-01-31"),
                ("limit", "2"),
                ("offset", "1"),
                ("sortBy", "publishedDate"),
                ("source", "reuters"),
                ("startDate", "2024-01-01"),
                ("tags", "earnings"),
                ("tickers", "AAPL"),
            ],
        },
        ToolCase {
            name: "get_fundamentals_definitions",
            route: "/tiingo/fundamentals/definitions",
            arguments: serde_json::json!({}),
            response: serde_json::json!([{"dataCode": "revenue", "description": "Total revenue"}]),
            query: &[],
        },
        ToolCase {
            name: "get_financial_statements",
            route: "/tiingo/fundamentals/AAPL/statements",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31"
            }),
            response: serde_json::json!([{"date": "2024-01-31", "quarter": 1}]),
            query: &[("endDate", "2024-01-31"), ("startDate", "2024-01-01")],
        },
        ToolCase {
            name: "get_daily_fundamentals",
            route: "/tiingo/fundamentals/AAPL/daily",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31"
            }),
            response: serde_json::json!([{"date": "2024-01-31", "marketCap": 2900000000000_i64}]),
            query: &[("endDate", "2024-01-31"), ("startDate", "2024-01-01")],
        },
        ToolCase {
            name: "get_company_meta",
            route: "/tiingo/fundamentals/meta",
            arguments: serde_json::json!({"tickers": "AAPL,MSFT"}),
            response: serde_json::json!([{"ticker": "AAPL", "sector": "Technology"}]),
            query: &[("tickers", "AAPL,MSFT")],
        },
        ToolCase {
            name: "get_dividends",
            route: "/tiingo/corporate-actions/AAPL/distributions",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31"
            }),
            response: serde_json::json!([{"ticker": "AAPL", "exDate": "2024-01-01", "distribution": 0.24}]),
            query: &[("endExDate", "2024-01-31"), ("startExDate", "2024-01-01")],
        },
        ToolCase {
            name: "get_dividend_yield",
            route: "/tiingo/corporate-actions/AAPL/distribution-yield",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31"
            }),
            response: serde_json::json!([{"date": "2024-01-31", "trailing12MoYield": 0.005}]),
            query: &[("endDate", "2024-01-31"), ("startDate", "2024-01-01")],
        },
        ToolCase {
            name: "get_splits",
            route: "/tiingo/corporate-actions/AAPL/splits",
            arguments: serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31"
            }),
            response: serde_json::json!([{"ticker": "AAPL", "exDate": "2024-01-15", "splitFactor": 2.0}]),
            query: &[("endExDate", "2024-01-31"), ("startExDate", "2024-01-01")],
        },
    ]
}

struct ExpectedToolSchema {
    name: &'static str,
    properties: &'static [&'static str],
    required: &'static [&'static str],
    optional: &'static [&'static str],
}

const EXPECTED_TOOL_SCHEMAS: [ExpectedToolSchema; 28] = [
    ExpectedToolSchema {
        name: "get_stock_metadata",
        properties: &["ticker"],
        required: &["ticker"],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "get_stock_prices",
        properties: &["ticker", "start_date", "end_date", "resample_freq"],
        required: &["ticker"],
        optional: &["start_date", "end_date", "resample_freq"],
    },
    ExpectedToolSchema {
        name: "get_realtime_price",
        properties: &["ticker", "after_hours"],
        required: &["ticker"],
        optional: &["after_hours"],
    },
    ExpectedToolSchema {
        name: "get_intraday_prices",
        properties: &[
            "ticker",
            "start_date",
            "end_date",
            "resample_freq",
            "columns",
        ],
        required: &["ticker"],
        optional: &["start_date", "end_date", "resample_freq", "columns"],
    },
    ExpectedToolSchema {
        name: "get_forex_quote",
        properties: &["ticker"],
        required: &["ticker"],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "get_forex_prices",
        properties: &["ticker", "start_date", "end_date", "resample_freq"],
        required: &["ticker"],
        optional: &["start_date", "end_date", "resample_freq"],
    },
    ExpectedToolSchema {
        name: "get_crypto_quote",
        properties: &["tickers"],
        required: &[],
        optional: &["tickers"],
    },
    ExpectedToolSchema {
        name: "get_crypto_prices",
        properties: &["tickers", "start_date", "end_date", "resample_freq"],
        required: &["tickers"],
        optional: &["start_date", "end_date", "resample_freq"],
    },
    ExpectedToolSchema {
        name: "get_crypto_metadata",
        properties: &["tickers"],
        required: &[],
        optional: &["tickers"],
    },
    ExpectedToolSchema {
        name: "get_news",
        properties: &[
            "tickers",
            "tags",
            "source",
            "start_date",
            "end_date",
            "limit",
            "offset",
            "sort_by",
        ],
        required: &[],
        optional: &[
            "tickers",
            "tags",
            "source",
            "start_date",
            "end_date",
            "limit",
            "offset",
            "sort_by",
        ],
    },
    ExpectedToolSchema {
        name: "get_fundamentals_definitions",
        properties: &[],
        required: &[],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "get_financial_statements",
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
    ExpectedToolSchema {
        name: "get_daily_fundamentals",
        properties: &["ticker", "start_date", "end_date", "columns"],
        required: &["ticker"],
        optional: &["start_date", "end_date", "columns"],
    },
    ExpectedToolSchema {
        name: "get_company_meta",
        properties: &["tickers", "columns"],
        required: &["tickers"],
        optional: &["columns"],
    },
    ExpectedToolSchema {
        name: "get_dividends",
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
    ExpectedToolSchema {
        name: "get_dividend_yield",
        properties: &["ticker", "start_date", "end_date", "columns"],
        required: &["ticker"],
        optional: &["start_date", "end_date", "columns"],
    },
    ExpectedToolSchema {
        name: "get_splits",
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
    ExpectedToolSchema {
        name: "get_equity_realtime_snapshot",
        properties: &["ticker"],
        required: &[],
        optional: &["ticker"],
    },
    ExpectedToolSchema {
        name: "get_equity_intraday_prices",
        properties: &[
            "ticker",
            "start_date",
            "end_date",
            "resample_freq",
            "after_hours",
            "force_fill",
            "columns",
        ],
        required: &["ticker"],
        optional: &[
            "start_date",
            "end_date",
            "resample_freq",
            "after_hours",
            "force_fill",
            "columns",
        ],
    },
    ExpectedToolSchema {
        name: "get_boats_snapshot",
        properties: &["ticker"],
        required: &[],
        optional: &["ticker"],
    },
    ExpectedToolSchema {
        name: "get_boats_prices",
        properties: &[
            "ticker",
            "start_date",
            "end_date",
            "resample_freq",
            "after_hours",
            "columns",
        ],
        required: &["ticker"],
        optional: &[
            "start_date",
            "end_date",
            "resample_freq",
            "after_hours",
            "columns",
        ],
    },
    ExpectedToolSchema {
        name: "get_fund_metadata",
        properties: &["ticker"],
        required: &["ticker"],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "get_fund_fee_metrics",
        properties: &["ticker"],
        required: &["ticker"],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "search_tiingo_assets",
        properties: &["query"],
        required: &["query"],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "get_crypto_yield_platforms",
        properties: &["platform_codes"],
        required: &[],
        optional: &["platform_codes"],
    },
    ExpectedToolSchema {
        name: "get_crypto_yield_pools",
        properties: &["pool_codes", "platform_codes"],
        required: &[],
        optional: &["pool_codes", "platform_codes"],
    },
    ExpectedToolSchema {
        name: "get_crypto_yield_ticks",
        properties: &["pool_codes", "platform_codes"],
        required: &[],
        optional: &["pool_codes", "platform_codes"],
    },
    ExpectedToolSchema {
        name: "get_crypto_yield_metrics",
        properties: &["pool_code", "start_date", "end_date", "resample_freq"],
        required: &["pool_code"],
        optional: &["start_date", "end_date", "resample_freq"],
    },
];

#[tokio::test]
async fn preserves_legacy_tool_descriptors_and_discovers_additive_typed_tools() {
    let upstream = MockServer::start().await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    let tools = connection.client.list_tools(None).await.unwrap().tools;
    let actual_names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<BTreeSet<_>>();
    let expected_names = TOOL_NAMES.into_iter().collect::<BTreeSet<_>>();

    assert_eq!(tools.len(), 34);
    assert!(expected_names.is_subset(&actual_names));
    assert!(actual_names.contains("get_bulk_eod_prices"));
    assert!(actual_names.contains("get_ticker_metadata"));
    assert!(actual_names.contains("get_iex_market_snapshot"));
    assert!(actual_names.contains("get_forex_quotes"));
    assert!(actual_names.contains("get_distributions_by_ex_date"));
    assert!(actual_names.contains("get_splits_by_ex_date"));
    assert!(actual_names.contains("get_equity_realtime_snapshot"));
    assert!(actual_names.contains("get_equity_intraday_prices"));
    assert!(actual_names.contains("get_boats_snapshot"));
    assert!(actual_names.contains("get_boats_prices"));
    assert!(actual_names.contains("get_fund_metadata"));
    assert!(actual_names.contains("get_fund_fee_metrics"));
    assert!(actual_names.contains("search_tiingo_assets"));
    assert!(actual_names.contains("get_crypto_yield_platforms"));
    assert!(actual_names.contains("get_crypto_yield_pools"));
    assert!(actual_names.contains("get_crypto_yield_ticks"));
    assert!(actual_names.contains("get_crypto_yield_metrics"));

    for tool in &tools {
        assert!(
            !tool.input_schema.contains_key("$schema"),
            "{} advertised an unapproved $schema field",
            tool.name
        );
        assert_eq!(
            serde_json::to_value(&tool.meta).unwrap(),
            serde_json::json!({"fastmcp": {"tags": []}}),
            "{} discovery metadata drifted",
            tool.name
        );
    }

    for expected in EXPECTED_TOOL_SCHEMAS {
        let tool = tools
            .iter()
            .find(|tool| tool.name == expected.name)
            .unwrap();
        let properties = tool.input_schema["properties"].as_object().unwrap();
        assert_eq!(
            properties
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected.properties.iter().copied().collect(),
            "{} property names drifted",
            expected.name
        );
        assert_eq!(
            tool.input_schema
                .get("required")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .map(|field| field.as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            expected.required.iter().copied().collect(),
            "{} required fields drifted",
            expected.name
        );
        assert_eq!(
            tool.input_schema["additionalProperties"],
            serde_json::json!(false),
            "{} must reject unknown arguments",
            expected.name
        );
        for field in expected.optional {
            assert_eq!(
                properties[*field].get("default"),
                Some(&serde_json::Value::Null),
                "{}.{} must advertise its null default",
                expected.name,
                field
            );
        }
        for field in expected.required {
            assert!(
                properties[*field].get("default").is_none(),
                "{}.{} must remain required without a default",
                expected.name,
                field
            );
        }
    }

    let news = tools.iter().find(|tool| tool.name == "get_news").unwrap();
    assert_eq!(
        news.input_schema["properties"]["limit"]["type"],
        serde_json::json!(["integer", "null"])
    );
    assert_eq!(
        news.input_schema["properties"]["sort_by"]["type"],
        serde_json::json!(["string", "null"])
    );

    let stock_prices = tools
        .iter()
        .find(|tool| tool.name == "get_stock_prices")
        .unwrap();
    assert_eq!(
        stock_prices.input_schema["required"],
        serde_json::json!(["ticker"])
    );
    assert_eq!(
        stock_prices.input_schema["properties"]["resample_freq"]["type"],
        serde_json::json!(["string", "null"])
    );

    for name in [
        "get_intraday_prices",
        "get_daily_fundamentals",
        "get_company_meta",
        "get_dividend_yield",
    ] {
        let tool = tools.iter().find(|tool| tool.name == name).unwrap();
        assert_eq!(
            tool.input_schema["properties"]["columns"]["type"],
            serde_json::json!(["array", "null"]),
            "{name} must advertise optional columns"
        );
        assert_eq!(
            tool.input_schema["properties"]["columns"]["items"]["type"],
            serde_json::json!("string"),
            "{name} must accept string column identifiers"
        );
    }

    connection.close().await;
}

#[tokio::test]
async fn additive_task_two_tools_preserve_json_text_and_structured_data() {
    let upstream = MockServer::start().await;
    let snapshot = serde_json::json!([{"ticker": "AAPL", "tngoLast": 227.16}]);
    let forex = serde_json::json!([{"ticker": "eurusd", "midPrice": 1.0812}]);
    let distributions = serde_json::json!([{
        "ticker": "SPY",
        "exDate": "2027-02-15",
        "announcedDate": "2027-01-10",
        "distribution": 1.23
    }]);
    let splits = serde_json::json!([{
        "ticker": "XYZ",
        "exDate": "2027-03-01",
        "announcedDate": "2027-02-01",
        "isCancelled": true,
        "splitFactor": 1.5
    }]);
    for (route, query, response) in [
        ("/iex", None, snapshot.clone()),
        (
            "/tiingo/fx/top",
            Some(("tickers", "eurusd,gbpusd")),
            forex.clone(),
        ),
        (
            "/tiingo/corporate-actions/distributions",
            Some(("exDate", "2027-02-15")),
            distributions.clone(),
        ),
        (
            "/tiingo/corporate-actions/splits",
            Some(("exDate", "2027-03-01")),
            splits.clone(),
        ),
    ] {
        let mut mock = Mock::given(method("GET")).and(path(route));
        if let Some((name, value)) = query {
            mock = mock.and(query_param(name, value));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&upstream)
            .await;
    }
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    for (name, call_arguments, expected) in [
        ("get_iex_market_snapshot", serde_json::json!({}), snapshot),
        (
            "get_forex_quotes",
            serde_json::json!({"tickers": ["EURUSD", "gbpusd"]}),
            forex,
        ),
        (
            "get_distributions_by_ex_date",
            serde_json::json!({"ex_date": "2027-02-15"}),
            distributions,
        ),
        (
            "get_splits_by_ex_date",
            serde_json::json!({"ex_date": "2027-03-01"}),
            splits,
        ),
    ] {
        let result = connection
            .client
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(call_arguments)))
            .await
            .unwrap();
        assert_accurate_success(name, &result, &expected);
    }

    connection.close().await;
}

#[tokio::test]
async fn additive_task_three_tools_preserve_json_text_and_structured_data() {
    let upstream = MockServer::start().await;
    let equity_snapshot = serde_json::json!([{"ticker": "AAPL", "last": 227.16}]);
    let equity_prices = serde_json::json!([{
        "date": "2024-01-02T14:30:00Z",
        "ticker": "AAPL",
        "close": 227.16
    }]);
    let boats_snapshot = serde_json::json!([{"ticker": "AAPL", "last": 226.98}]);
    let boats_prices = serde_json::json!([{
        "date": "2024-01-02T23:30:00Z",
        "ticker": "AAPL",
        "close": 226.98
    }]);

    for route in [
        "/tiingo/equity/intraday",
        "/tiingo/equity/intraday/AAPL",
        "/boats",
        "/boats/AAPL",
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                if route.starts_with("/boats") {
                    boats_snapshot.clone()
                } else {
                    equity_snapshot.clone()
                },
            ))
            .expect(1)
            .mount(&upstream)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/tiingo/equity/intraday/AAPL/prices"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("resampleFreq", "5min"))
        .and(query_param("afterHours", "true"))
        .and(query_param("forceFill", "true"))
        .and(query_param("columns", "date,close"))
        .respond_with(ResponseTemplate::new(200).set_body_json(equity_prices.clone()))
        .expect(1)
        .mount(&upstream)
        .await;
    Mock::given(method("GET"))
        .and(path("/boats/AAPL/prices"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("resampleFreq", "1hour"))
        .and(query_param("afterHours", "false"))
        .and(query_param("columns", "ticker,close"))
        .respond_with(ResponseTemplate::new(200).set_body_json(boats_prices.clone()))
        .expect(1)
        .mount(&upstream)
        .await;

    let connection = Connection::new(test_client(&upstream, "test-key")).await;
    for (name, call_arguments, expected) in [
        (
            "get_equity_realtime_snapshot",
            serde_json::json!({}),
            equity_snapshot.clone(),
        ),
        (
            "get_equity_realtime_snapshot",
            serde_json::json!({"ticker": "AAPL"}),
            equity_snapshot,
        ),
        (
            "get_equity_intraday_prices",
            serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "5min",
                "after_hours": true,
                "force_fill": true,
                "columns": ["date", "close"]
            }),
            equity_prices,
        ),
        (
            "get_boats_snapshot",
            serde_json::json!({}),
            boats_snapshot.clone(),
        ),
        (
            "get_boats_snapshot",
            serde_json::json!({"ticker": "AAPL"}),
            boats_snapshot,
        ),
        (
            "get_boats_prices",
            serde_json::json!({
                "ticker": "AAPL",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "1hour",
                "after_hours": false,
                "columns": ["ticker", "close"]
            }),
            boats_prices,
        ),
    ] {
        let result = connection
            .client
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(call_arguments)))
            .await
            .unwrap();
        assert_accurate_success(name, &result, &expected);
    }

    connection.close().await;
}

#[tokio::test]
async fn additive_task_four_tools_preserve_json_text_and_structured_data() {
    let upstream = MockServer::start().await;
    let fund_metadata = serde_json::json!({"ticker": "VFIAX", "name": "Vanguard 500 Index Fund"});
    let fund_metrics = serde_json::json!([{"prospectusDate": "2024-01-01", "netExpense": 0.0004}]);
    let search = serde_json::json!([{"ticker": "AAPL", "name": "Apple Inc.", "isActive": true}]);
    let platforms = serde_json::json!([{"platformCode": "AAVEV2", "network": "ETH"}]);
    let pools = serde_json::json!([{"poolCode": "aavev2_usdc", "yieldPlatform": "AAVEV2"}]);
    let ticks = serde_json::json!([{"poolCode": "aavev2_usdc", "supplyRate": 0.04}]);
    let metrics = serde_json::json!([{"date": "2024-01-01T00:00:00Z", "closeSupplyRate": 0.04}]);

    for (route, query, response) in [
        ("/tiingo/funds/VFIAX", None, fund_metadata.clone()),
        ("/tiingo/funds/VFIAX/metrics", None, fund_metrics.clone()),
        (
            "/tiingo/utilities/search",
            Some(("query", "Apple")),
            search.clone(),
        ),
        (
            "/tiingo/crypto-yield/platforms",
            Some(("platformCodes", "AAVEV2,COMPOUND")),
            platforms.clone(),
        ),
        (
            "/tiingo/crypto-yield/pools",
            Some(("poolCodes", "aavev2_usdc,compound_usdc")),
            pools.clone(),
        ),
        (
            "/tiingo/crypto-yield/ticks",
            Some(("platformCodes", "AAVEV2")),
            ticks.clone(),
        ),
        (
            "/tiingo/crypto-yield/aavev2_usdc/metrics",
            Some(("resampleFreq", "5min")),
            metrics.clone(),
        ),
    ] {
        let mut mock = Mock::given(method("GET")).and(path(route));
        if let Some((name, value)) = query {
            mock = mock.and(query_param(name, value));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&upstream)
            .await;
    }

    let connection = Connection::new(test_client(&upstream, "test-key")).await;
    for (name, call_arguments, expected) in [
        (
            "get_fund_metadata",
            serde_json::json!({"ticker": "VFIAX"}),
            fund_metadata,
        ),
        (
            "get_fund_fee_metrics",
            serde_json::json!({"ticker": "VFIAX"}),
            fund_metrics,
        ),
        (
            "search_tiingo_assets",
            serde_json::json!({"query": "  Apple  "}),
            search,
        ),
        (
            "get_crypto_yield_platforms",
            serde_json::json!({"platform_codes": ["AAVEV2", "COMPOUND"]}),
            platforms,
        ),
        (
            "get_crypto_yield_pools",
            serde_json::json!({"pool_codes": ["aavev2_usdc", "compound_usdc"]}),
            pools,
        ),
        (
            "get_crypto_yield_ticks",
            serde_json::json!({"platform_codes": ["AAVEV2"]}),
            ticks,
        ),
        (
            "get_crypto_yield_metrics",
            serde_json::json!({
                "pool_code": "aavev2_usdc",
                "start_date": "2024-01-01",
                "end_date": "2024-01-31",
                "resample_freq": "5min"
            }),
            metrics,
        ),
    ] {
        let result = connection
            .client
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(call_arguments)))
            .await
            .unwrap();
        assert_accurate_success(name, &result, &expected);
    }

    connection.close().await;
}

#[tokio::test]
async fn legacy_column_extensions_preserve_json_text_and_structured_data() {
    let upstream = MockServer::start().await;
    let expected = serde_json::json!([{"ticker": "AAPL", "value": 1}]);
    for (route, columns) in [
        ("/iex/AAPL/prices", "ticker,close"),
        ("/tiingo/fundamentals/AAPL/daily", "marketCap,peRatio"),
        ("/tiingo/fundamentals/meta", "ticker,sector"),
        (
            "/tiingo/corporate-actions/AAPL/distribution-yield",
            "trailing12MoYield",
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .and(query_param("columns", columns))
            .respond_with(ResponseTemplate::new(200).set_body_json(expected.clone()))
            .expect(1)
            .mount(&upstream)
            .await;
    }
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    for (name, call_arguments) in [
        (
            "get_intraday_prices",
            serde_json::json!({"ticker": "AAPL", "columns": ["ticker", "close"]}),
        ),
        (
            "get_daily_fundamentals",
            serde_json::json!({"ticker": "AAPL", "columns": ["marketCap", "peRatio"]}),
        ),
        (
            "get_company_meta",
            serde_json::json!({"tickers": "AAPL", "columns": ["ticker", "sector"]}),
        ),
        (
            "get_dividend_yield",
            serde_json::json!({"ticker": "AAPL", "columns": ["trailing12MoYield"]}),
        ),
    ] {
        let result = connection
            .client
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(call_arguments)))
            .await
            .unwrap();
        assert_accurate_success(name, &result, &expected);
    }

    connection.close().await;
}

#[tokio::test]
async fn bulk_eod_prices_preserves_json_text_and_structured_data() {
    let upstream = MockServer::start().await;
    let expected = serde_json::json!({
        "prices": [{
            "date": "2024-01-02",
            "ticker": "AAPL",
            "open": 100.0,
            "high": 105.0,
            "low": 99.0,
            "close": 104.0,
            "volume": 1000.0,
            "adjOpen": 100.0,
            "adjHigh": 105.0,
            "adjLow": 99.0,
            "adjClose": 104.0,
            "adjVolume": 1000.0,
            "divCash": 0.0,
            "splitFactor": 1.0
        }],
        "historyRefreshTickers": []
    });
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/prices"))
        .and(query_param("format", "csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(concat!(
            "date,ticker,open,high,low,close,volume,adjOpen,adjHigh,adjLow,adjClose,adjVolume,divCash,splitFactor\n",
            "2024-01-02,AAPL,100,105,99,104,1000,100,105,99,104,1000,0,1\n"
        )))
        .expect(1)
        .mount(&upstream)
        .await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    let tool = connection
        .client
        .list_tools(None)
        .await
        .unwrap()
        .tools
        .into_iter()
        .find(|tool| tool.name == "get_bulk_eod_prices")
        .expect("bulk EOD prices must be discoverable");
    assert!(
        tool.input_schema["properties"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(tool.input_schema["additionalProperties"], false);
    let result = connection
        .client
        .call_tool(
            CallToolRequestParams::new("get_bulk_eod_prices")
                .with_arguments(arguments(serde_json::json!({}))),
        )
        .await
        .unwrap();

    assert_accurate_success("get_bulk_eod_prices", &result, &expected);
    connection.close().await;
}

#[tokio::test]
async fn ticker_metadata_requires_columns_and_preserves_json_text_and_structured_data() {
    let upstream = MockServer::start().await;
    let expected = serde_json::json!([{
        "ticker": "AAPL",
        "permaTicker": "AAPL",
        "openfigi": "BBG000B9XRY4"
    }]);
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/meta"))
        .and(query_param("columns", "ticker,permaTicker,openfigi"))
        .respond_with(ResponseTemplate::new(200).set_body_json(expected.clone()))
        .expect(1)
        .mount(&upstream)
        .await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    let tool = connection
        .client
        .list_tools(None)
        .await
        .unwrap()
        .tools
        .into_iter()
        .find(|tool| tool.name == "get_ticker_metadata")
        .expect("ticker metadata must be discoverable");
    assert_eq!(
        tool.input_schema["required"],
        serde_json::json!(["columns"])
    );
    assert_eq!(
        tool.input_schema["properties"]["columns"]["type"],
        serde_json::json!("array")
    );
    assert_eq!(tool.input_schema["additionalProperties"], false);

    let missing = connection
        .client
        .call_tool(
            CallToolRequestParams::new("get_ticker_metadata")
                .with_arguments(arguments(serde_json::json!({}))),
        )
        .await
        .unwrap();
    assert_eq!(missing.is_error, Some(true));
    assert!(upstream.received_requests().await.unwrap().is_empty());

    let result = connection
        .client
        .call_tool(
            CallToolRequestParams::new("get_ticker_metadata").with_arguments(arguments(
                serde_json::json!({"columns": ["ticker", "permaTicker", "openfigi"]}),
            )),
        )
        .await
        .unwrap();

    assert_accurate_success("get_ticker_metadata", &result, &expected);
    connection.close().await;
}

#[tokio::test]
async fn rejects_unknown_arguments_before_contacting_tiingo() {
    let upstream = MockServer::start().await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    for (name, call_arguments) in [
        (
            "get_stock_metadata",
            serde_json::json!({"ticker": "AAPL", "unexpected": true}),
        ),
        (
            "get_fundamentals_definitions",
            serde_json::json!({"unexpected": true}),
        ),
    ] {
        let result = connection
            .client
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments(call_arguments)))
            .await
            .unwrap();

        assert_eq!(
            result.is_error,
            Some(true),
            "{name} accepted an unknown field"
        );
        assert!(
            result.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("unknown field"),
            "{name} did not report the invalid argument"
        );
    }

    assert!(upstream.received_requests().await.unwrap().is_empty());
    connection.close().await;
}

#[tokio::test]
async fn returns_legacy_pretty_json_and_structured_success_content() {
    let upstream = MockServer::start().await;
    let value = serde_json::json!([{"ticker": "AAPL"}]);
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL"))
        .respond_with(ResponseTemplate::new(200).set_body_json(value.clone()))
        .expect(1)
        .mount(&upstream)
        .await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    let result = connection
        .client
        .call_tool(
            CallToolRequestParams::new("get_stock_metadata")
                .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"}))),
        )
        .await
        .unwrap();

    let text = &result.content[0].as_text().unwrap().text;
    assert!(text.contains('\n'));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).unwrap(),
        value
    );
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.structured_content,
        Some(serde_json::json!({
            "data": [{"ticker": "AAPL"}],
            "meta": {"source": "tiingo"}
        }))
    );

    connection.close().await;
}

#[tokio::test]
async fn returns_sanitized_structured_authentication_errors() {
    let upstream = MockServer::start().await;
    let secret = "credential-that-must-not-leak";
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(format!("Authorization: Token {secret}")),
        )
        .expect(1)
        .mount(&upstream)
        .await;
    let connection = Connection::new(test_client(&upstream, secret)).await;

    let result = connection
        .client
        .call_tool(
            CallToolRequestParams::new("get_stock_metadata")
                .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"}))),
        )
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = result.content[0]
        .as_text()
        .expect("recoverable errors retain a JSON text block");
    let text_payload = serde_json::from_str::<serde_json::Value>(&text.text).unwrap();
    assert_eq!(text_payload["kind"], "authentication");
    assert_eq!(
        text_payload,
        result.structured_content.as_ref().unwrap()["error"]
    );
    assert_eq!(
        result.structured_content.as_ref().unwrap()["error"]["kind"],
        "authentication"
    );
    assert!(!serde_json::to_string(&result).unwrap().contains(secret));
    assert!(!serde_json::to_string(&result).unwrap().contains("Token "));

    connection.close().await;
}

#[tokio::test]
async fn rejects_negative_news_pagination_before_contacting_tiingo() {
    let upstream = MockServer::start().await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    for invalid_arguments in [
        serde_json::json!({"limit": -1}),
        serde_json::json!({"offset": -1}),
    ] {
        let result = connection
            .client
            .call_tool(
                CallToolRequestParams::new("get_news").with_arguments(arguments(invalid_arguments)),
            )
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("failed to deserialize parameters")
        );
    }

    assert!(upstream.received_requests().await.unwrap().is_empty());
    connection.close().await;
}

#[tokio::test]
async fn every_tool_preserves_data_and_forwards_exact_arguments() {
    let upstream = MockServer::start().await;
    let cases = all_tool_cases();

    for case in &cases {
        let mut mock = Mock::given(method("GET")).and(path(case.route));
        if case.name == "get_crypto_quote" {
            mock = mock.and(query_param("tickers", "btcusd"));
        } else if case.name == "get_crypto_prices" {
            mock = mock.and(query_param("tickers", "btcusd,ethusd"));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(case.response.clone()))
            .expect(1)
            .mount(&upstream)
            .await;
    }

    let connection = Connection::new(test_client(&upstream, "test-key")).await;
    for (index, case) in cases.iter().enumerate() {
        let result = connection
            .client
            .call_tool(
                CallToolRequestParams::new(case.name)
                    .with_arguments(arguments(case.arguments.clone())),
            )
            .await
            .unwrap();
        assert_accurate_success(case.name, &result, &case.response);

        let requests = upstream.received_requests().await.unwrap();
        assert_eq!(
            requests.len(),
            index + 1,
            "{} made an unexpected number of upstream requests",
            case.name
        );
        let request = requests.last().unwrap();
        assert_eq!(
            request.url.path(),
            case.route,
            "{} used the wrong route",
            case.name
        );
        let mut query = request
            .url
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        query.sort_unstable();
        let mut expected_query = case
            .query
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>();
        expected_query.sort_unstable();
        assert_eq!(query, expected_query, "{} changed its query", case.name);
    }

    connection.close().await;
}

#[tokio::test]
async fn repeated_eod_calls_are_consistent_accurate_and_fast() {
    const SAMPLES: usize = 30;
    const P95_LIMIT: Duration = Duration::from_millis(250);

    let upstream = MockServer::start().await;
    let documented_eod_shape = serde_json::json!([{
        "date": "2026-08-24T00:00:00.000Z",
        "open": 227.15,
        "high": 229.89,
        "low": 224.42,
        "close": 227.16,
        "volume": 34567890,
        "adjOpen": 226.89,
        "adjHigh": 229.63,
        "adjLow": 224.16,
        "adjClose": 226.90,
        "adjVolume": 34567890,
        "divCash": 0.26,
        "splitFactor": 1.0
    }]);
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL/prices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(documented_eod_shape.clone()))
        .expect((SAMPLES + 1) as u64)
        .mount(&upstream)
        .await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;
    let request = || {
        CallToolRequestParams::new("get_stock_prices")
            .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"})))
    };

    let warmup = connection.client.call_tool(request()).await.unwrap();
    assert_accurate_success("get_stock_prices", &warmup, &documented_eod_shape);

    let mut latencies = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let result = tokio::time::timeout(P95_LIMIT * 2, connection.client.call_tool(request()))
            .await
            .expect("an MCP tool call exceeded 500 ms")
            .unwrap();
        latencies.push(started.elapsed());
        assert_accurate_success("get_stock_prices", &result, &documented_eod_shape);
    }
    latencies.sort_unstable();
    let median = latencies[SAMPLES / 2];
    let p95 = latencies[(SAMPLES * 95).div_ceil(100) - 1];
    eprintln!("MCP EOD latency: median={median:?}, p95={p95:?}, samples={SAMPLES}");
    assert!(
        p95 < P95_LIMIT,
        "MCP EOD p95 latency {p95:?} exceeded {P95_LIMIT:?}"
    );

    connection.close().await;
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY and consumes one EOD request"]
async fn live_mcp_eod_data_is_consistent_accurate_and_timely() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    anyhow::ensure!(config.api_key.is_some(), "TIINGO_API_KEY is required");
    let connection = Connection::new(TiingoClient::new(config)?).await;
    let request = CallToolRequestParams::new("get_stock_prices").with_arguments(arguments(
        serde_json::json!({
            "ticker": "AAPL",
            "start_date": "2024-01-02",
            "end_date": "2024-01-02"
        }),
    ));

    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        connection.client.call_tool(request),
    )
    .await
    .expect("live MCP EOD request exceeded 10 seconds")?;
    let elapsed = started.elapsed();
    anyhow::ensure!(
        result.is_error == Some(false),
        "live MCP EOD request failed"
    );

    let structured = result
        .structured_content
        .as_ref()
        .context("live MCP response omitted structured content")?;
    anyhow::ensure!(structured["meta"]["source"] == "tiingo");
    let rows = structured["data"]
        .as_array()
        .context("live MCP EOD data was not an array")?;
    anyhow::ensure!(!rows.is_empty(), "live MCP EOD response was empty");
    for row in rows {
        let open = row["open"].as_f64().context("open was not numeric")?;
        let high = row["high"].as_f64().context("high was not numeric")?;
        let low = row["low"].as_f64().context("low was not numeric")?;
        let close = row["close"].as_f64().context("close was not numeric")?;
        anyhow::ensure!(high >= open && high >= close && high >= low);
        anyhow::ensure!(low <= open && low <= close && low <= high);
        anyhow::ensure!(row["volume"].as_u64().is_some(), "volume was not unsigned");
        for field in [
            "date",
            "adjOpen",
            "adjHigh",
            "adjLow",
            "adjClose",
            "adjVolume",
            "divCash",
            "splitFactor",
        ] {
            anyhow::ensure!(!row[field].is_null(), "{field} was missing");
        }
    }

    let text = &result.content[0]
        .as_text()
        .context("live MCP response omitted JSON text")?
        .text;
    anyhow::ensure!(serde_json::from_str::<serde_json::Value>(text)? == structured["data"]);
    eprintln!(
        "live MCP EOD latency: {elapsed:?}, rows={}, ticker=AAPL, date=2024-01-02",
        rows.len()
    );

    connection.close().await;
    Ok(())
}
