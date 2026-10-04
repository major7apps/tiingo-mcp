use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    mcp::TiingoServer,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

async fn call(name: &str, args: Value, upstream: &MockServer) -> rmcp::model::CallToolResult {
    let tiingo = TiingoClient::new(Config {
        api_key: Some("fixture-key".into()),
        base_url: upstream.uri().parse().unwrap(),
        request_timeout: std::time::Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    })
    .unwrap();
    let (input, output) = tokio::io::duplex(16 * 1024);
    let server = tokio::spawn(async move {
        TiingoServer::with_client(tiingo)
            .serve(input)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let mut client = ().serve(output).await.unwrap();
    let result = client
        .call_tool(
            CallToolRequestParams::new(name.to_owned())
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    client.close().await.unwrap();
    server.await.unwrap();
    result
}

#[tokio::test]
async fn documented_rest_controls_reach_the_exact_upstream_queries() {
    let cases = [
        (
            "get_stock_prices",
            json!({"ticker":"AAPL","sort":"-date","columns":["date","adjClose"]}),
            "/tiingo/daily/AAPL/prices",
            vec![("sort", "-date"), ("columns", "date,adjClose")],
        ),
        (
            "get_financial_statements",
            json!({"ticker":"AAPL","as_reported":true,"sort":"date"}),
            "/tiingo/fundamentals/AAPL/statements",
            vec![("asReported", "true"), ("sort", "date")],
        ),
        (
            "get_financial_statements",
            json!({"ticker":"AAPL","as_reported":false}),
            "/tiingo/fundamentals/AAPL/statements",
            vec![("asReported", "false")],
        ),
        (
            "get_daily_fundamentals",
            json!({"ticker":"AAPL","sort":"-date"}),
            "/tiingo/fundamentals/AAPL/daily",
            vec![("sort", "-date")],
        ),
        (
            "search_tiingo_assets",
            json!({"query":" AAPL ","exact_ticker_match":true,"include_delisted":false,"limit":100}),
            "/tiingo/utilities/search",
            vec![
                ("query", "AAPL"),
                ("exactTickerMatch", "true"),
                ("includeDelisted", "false"),
                ("limit", "100"),
            ],
        ),
        (
            "get_intraday_prices",
            json!({"ticker":"AAPL","resample_freq":"4hour","after_hours":true,"force_fill":false}),
            "/iex/AAPL/prices",
            vec![
                ("resampleFreq", "4hour"),
                ("afterHours", "true"),
                ("forceFill", "false"),
            ],
        ),
        (
            "get_boats_prices",
            json!({"ticker":"AAPL","resample_freq":"2hour","force_fill":true}),
            "/boats/AAPL/prices",
            vec![("resampleFreq", "2hour"), ("forceFill", "true")],
        ),
        (
            "get_equity_intraday_prices",
            json!({"ticker":"AAPL","resample_freq":"45min"}),
            "/tiingo/equity/intraday/AAPL/prices",
            vec![("resampleFreq", "45min")],
        ),
        (
            "get_forex_prices",
            json!({"ticker":"eurusd","resample_freq":"4hour"}),
            "/tiingo/fx/eurusd/prices",
            vec![("resampleFreq", "4hour")],
        ),
        (
            "get_crypto_prices",
            json!({"tickers":"btcusd","resample_freq":"2day","exchanges":["BINANCE","POLONIEX"]}),
            "/tiingo/crypto/prices",
            vec![
                ("tickers", "btcusd"),
                ("resampleFreq", "2day"),
                ("exchanges", "BINANCE,POLONIEX"),
            ],
        ),
        (
            "get_crypto_quote",
            json!({"tickers":"btcusd","resample_freq":"4hour","exchanges":["BINANCE"]}),
            "/tiingo/crypto/prices",
            vec![
                ("tickers", "btcusd"),
                ("resampleFreq", "4hour"),
                ("exchanges", "BINANCE"),
            ],
        ),
        (
            "get_crypto_yield_metrics",
            json!({"pool_code":"aavev2_usdc","resample_freq":"3day"}),
            "/tiingo/crypto-yield/aavev2_usdc/metrics",
            vec![("resampleFreq", "3day")],
        ),
        (
            "get_iex_market_snapshot",
            json!({"tickers":[" AAPL ","SPY"]}),
            "/iex",
            vec![("tickers", "aapl,spy")],
        ),
        (
            "get_equity_realtime_snapshot",
            json!({"tickers":["AAPL","SPY"]}),
            "/tiingo/equity/intraday",
            vec![("tickers", "aapl,spy")],
        ),
        (
            "get_boats_snapshot",
            json!({"tickers":["AAPL","SPY"]}),
            "/boats",
            vec![("tickers", "aapl,spy")],
        ),
    ];
    for (name, args, route, query) in cases {
        let upstream = MockServer::start().await;
        let payload = json!([{"ticker":"AAPL","date":"2024-01-02","close":185.64}]);
        let mut mock = Mock::given(method("GET")).and(path(route));
        for (key, value) in &query {
            mock = mock.and(query_param(*key, *value));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(payload.clone()))
            .expect(1)
            .mount(&upstream)
            .await;
        let result = call(name, args, &upstream).await;
        assert_eq!(result.is_error, Some(false), "{name}: {result:?}");
        assert_eq!(
            result.structured_content.as_ref().unwrap()["data"],
            payload,
            "{name}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&result.content[0].as_text().unwrap().text).unwrap(),
            payload
        );
        let requests = upstream.received_requests().await.unwrap();
        assert_eq!(
            requests[0].url.query_pairs().count(),
            query.len(),
            "{name} added unexpected defaults"
        );
    }
}

#[tokio::test]
async fn invalid_rest_controls_fail_before_any_upstream_request() {
    let upstream = MockServer::start().await;
    let cases = [
        (
            "get_equity_realtime_snapshot",
            json!({"ticker":"AAPL","tickers":["SPY"]}),
        ),
        (
            "get_boats_snapshot",
            json!({"ticker":"AAPL","tickers":["SPY"]}),
        ),
        ("get_iex_market_snapshot", json!({"tickers":[]})),
        (
            "get_iex_market_snapshot",
            json!({"tickers":vec!["AAPL";101]}),
        ),
        ("search_tiingo_assets", json!({"query":"AAPL","limit":0})),
        ("search_tiingo_assets", json!({"query":"AAPL","limit":101})),
        (
            "get_crypto_quote",
            json!({"tickers":"btcusd","exchanges":[]}),
        ),
        (
            "get_crypto_quote",
            json!({"tickers":"btcusd","exchanges":["BINANCE,OTHER"]}),
        ),
        (
            "get_crypto_prices",
            json!({"tickers":"btcusd","exchanges":vec!["BINANCE";101]}),
        ),
        ("get_stock_prices", json!({"ticker":"AAPL","columns":[]})),
    ];
    for (name, args) in [
        ("get_stock_prices", json!({"ticker":"AAPL","sort":""})),
        (
            "get_stock_prices",
            json!({"ticker":"AAPL","sort":"date,close"}),
        ),
        (
            "get_daily_fundamentals",
            json!({"ticker":"AAPL","sort":"--date"}),
        ),
        (
            "get_financial_statements",
            json!({"ticker":"AAPL","sort":"revenue"}),
        ),
        (
            "get_equity_realtime_snapshot",
            json!({"tickers":["../bad"]}),
        ),
        ("get_boats_snapshot", json!({"tickers":[]})),
        (
            "get_boats_prices",
            json!({"ticker":"AAPL","resample_freq":"2day"}),
        ),
        (
            "get_equity_intraday_prices",
            json!({"ticker":"AAPL","resample_freq":"2day"}),
        ),
        (
            "get_forex_prices",
            json!({"ticker":"eurusd","resample_freq":"2day"}),
        ),
    ]
    .into_iter()
    .chain(cases)
    {
        let result = call(name, args, &upstream).await;
        assert_eq!(result.is_error, Some(true), "{name}");
        assert!(
            result.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("validation"),
            "{name}"
        );
    }
    assert!(upstream.received_requests().await.unwrap().is_empty());
}

#[test]
fn custom_frequencies_validate_positive_units_and_preserve_legacy_values() {
    use tiingo_mcp::client::query::{IexResample, IntradayResample};
    for frequency in [
        "1min", "5min", "15min", "30min", "1hour", "45min", "4hour", "720min",
    ] {
        let parsed: IexResample = serde_json::from_value(json!(frequency)).unwrap();
        assert_eq!(parsed.as_str(), frequency);
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(frequency));
    }
    for frequency in [
        "1min", "5min", "15min", "30min", "1hour", "1day", "3day", "4hour",
    ] {
        let parsed: IntradayResample = serde_json::from_value(json!(frequency)).unwrap();
        assert_eq!(parsed.as_str(), frequency);
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(frequency));
    }
    for frequency in [
        "0min",
        "-1hour",
        "1.5hour",
        "hour",
        " 4hour",
        "4HOUR",
        "01min",
        "999999999999999999999min",
    ] {
        assert!(
            serde_json::from_value::<IntradayResample>(json!(frequency)).is_err(),
            "{frequency}"
        );
    }
    assert!(serde_json::from_value::<IexResample>(json!("1day")).is_err());
}

#[tokio::test]
async fn representative_rest_failures_preserve_mcp_error_classification_and_redaction() {
    let cases = [
        (
            "get_stock_prices",
            json!({"ticker":"AAPL"}),
            401,
            "fixture-key",
            "authentication",
            1,
        ),
        (
            "get_financial_statements",
            json!({"ticker":"AAPL"}),
            403,
            "fixture-key",
            "entitlement",
            1,
        ),
        (
            "get_ticker_metadata",
            json!({"columns":["ticker"]}),
            404,
            "fixture-key",
            "not_found",
            1,
        ),
        (
            "search_tiingo_assets",
            json!({"query":"AAPL"}),
            429,
            "fixture-key",
            "rate_limit",
            3,
        ),
        (
            "get_crypto_quote",
            json!({"tickers":"btcusd"}),
            503,
            "fixture-key",
            "transient",
            3,
        ),
        (
            "get_forex_prices",
            json!({"ticker":"eurusd"}),
            418,
            "authorization: fixture-key",
            "upstream",
            1,
        ),
        (
            "get_crypto_yield_platforms",
            json!({}),
            200,
            "not json",
            "decode",
            1,
        ),
        (
            "get_bulk_eod_prices",
            json!({}),
            200,
            "date,ticker\n2024-01-02,AAPL\n",
            "decode",
            1,
        ),
    ];
    for (name, args, status, body, kind, attempts) in cases {
        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .expect(attempts)
            .mount(&upstream)
            .await;
        let result = call(name, args, &upstream).await;
        assert_eq!(result.is_error, Some(true), "{name}");
        let text = &result.content[0].as_text().unwrap().text;
        assert!(!text.contains("fixture-key"));
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["kind"], kind, "{name}");
        assert_eq!(
            result.structured_content.unwrap()["error"],
            payload,
            "{name}"
        );
        if status != 200 {
            assert_eq!(payload["status_code"], status);
        }
    }
}

#[tokio::test]
async fn response_size_failure_survives_the_csv_mcp_boundary() {
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/prices"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 8 * 1024 * 1024 + 1]))
        .expect(1)
        .mount(&upstream)
        .await;
    let result = call("get_bulk_eod_prices", json!({}), &upstream).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content.unwrap()["error"]["kind"],
        "response_too_large"
    );
}
