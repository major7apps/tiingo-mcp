use std::{collections::BTreeSet, time::Duration};

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
    matchers::{method, path},
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

struct ExpectedToolSchema {
    name: &'static str,
    properties: &'static [&'static str],
    required: &'static [&'static str],
    optional: &'static [&'static str],
}

const EXPECTED_TOOL_SCHEMAS: [ExpectedToolSchema; 17] = [
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
        properties: &["ticker", "start_date", "end_date", "resample_freq"],
        required: &["ticker"],
        optional: &["start_date", "end_date", "resample_freq"],
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
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
    ExpectedToolSchema {
        name: "get_company_meta",
        properties: &["tickers"],
        required: &["tickers"],
        optional: &[],
    },
    ExpectedToolSchema {
        name: "get_dividends",
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
    ExpectedToolSchema {
        name: "get_dividend_yield",
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
    ExpectedToolSchema {
        name: "get_splits",
        properties: &["ticker", "start_date", "end_date"],
        required: &["ticker"],
        optional: &["start_date", "end_date"],
    },
];

#[tokio::test]
async fn discovers_exactly_the_legacy_tools_with_typed_inputs() {
    let upstream = MockServer::start().await;
    let connection = Connection::new(test_client(&upstream, "test-key")).await;

    let tools = connection.client.list_tools(None).await.unwrap().tools;
    let actual_names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<BTreeSet<_>>();
    let expected_names = TOOL_NAMES.into_iter().collect::<BTreeSet<_>>();

    assert_eq!(tools.len(), TOOL_NAMES.len());
    assert_eq!(actual_names, expected_names);

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
