use std::time::Duration;

use chrono::NaiveDate;
use tiingo_mcp::{
    client::{
        TiingoClient,
        query::{DateRange, EodResample, IexResample, IntradayResample},
    },
    config::{Config, RetryPolicy},
    error::TiingoError,
};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn test_config(base_url: Url) -> Config {
    Config {
        api_key: Some("test-key".to_owned()),
        base_url,
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    }
}

fn populated_range() -> DateRange {
    DateRange {
        start_date: Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        end_date: Some(NaiveDate::from_ymd_opt(2024, 1, 31).unwrap()),
    }
}

#[tokio::test]
async fn market_client_methods_use_the_exact_tiingo_routes_and_queries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"route": "metadata"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL/prices"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("resampleFreq", "weekly"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"route": "eod"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/iex/AAPL"))
        .and(query_param("afterHours", "true"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"route": "realtime"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/iex/AAPL/prices"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("resampleFreq", "5min"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"route": "intraday"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/fx/eurusd/top"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"route": "quote"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/fx/eurusd/prices"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("resampleFreq", "1day"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"route": "forex"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    assert_eq!(
        client.get_stock_metadata("AAPL").await.unwrap(),
        serde_json::json!({"route": "metadata"})
    );
    assert_eq!(
        client
            .get_stock_prices("AAPL", populated_range(), Some(EodResample::Weekly))
            .await
            .unwrap(),
        serde_json::json!({"route": "eod"})
    );
    assert_eq!(
        client.get_realtime_price("AAPL", Some(true)).await.unwrap(),
        serde_json::json!({"route": "realtime"})
    );
    assert_eq!(
        client
            .get_intraday_prices("AAPL", populated_range(), Some(IexResample::FiveMinutes))
            .await
            .unwrap(),
        serde_json::json!({"route": "intraday"})
    );
    assert_eq!(
        client.get_forex_quote("eurusd").await.unwrap(),
        serde_json::json!({"route": "quote"})
    );
    assert_eq!(
        client
            .get_forex_prices("eurusd", populated_range(), Some(IntradayResample::OneDay))
            .await
            .unwrap(),
        serde_json::json!({"route": "forex"})
    );

    let mut routes = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .map(|request| {
            let mut query = request
                .url
                .query_pairs()
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect::<Vec<_>>();
            query.sort_unstable();
            (request.url.path().to_owned(), query)
        })
        .collect::<Vec<_>>();
    routes.sort_unstable();

    assert_eq!(
        routes,
        vec![
            (
                "/iex/AAPL".to_owned(),
                vec![("afterHours".to_owned(), "true".to_owned())]
            ),
            (
                "/iex/AAPL/prices".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("resampleFreq".to_owned(), "5min".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            ("/tiingo/daily/AAPL".to_owned(), vec![]),
            (
                "/tiingo/daily/AAPL/prices".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("resampleFreq".to_owned(), "weekly".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/fx/eurusd/prices".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("resampleFreq".to_owned(), "1day".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            ("/tiingo/fx/eurusd/top".to_owned(), vec![]),
        ]
    );
}

#[tokio::test]
async fn rejects_unsafe_path_symbols_before_requesting_tiingo() {
    let server = MockServer::start().await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    for ticker in [".", "..", "A/APL", "A?APL", "A#APL", "A%APL"] {
        let error = client.get_stock_metadata(ticker).await.unwrap_err();
        assert!(matches!(error, TiingoError::Validation(_)));
    }

    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn bulk_eod_prices_use_the_csv_route_and_preserve_raw_adjusted_fields() {
    let server = MockServer::start().await;
    let csv = concat!(
        "date,ticker,open,high,low,close,volume,adjOpen,adjHigh,adjLow,adjClose,adjVolume,divCash,splitFactor\n",
        "2024-01-02,AAPL,100,105,99,104,1000,50,52.5,49.5,52,2000,0,2\n",
        "2024-01-02,MSFT,200,205,199,204,3000,200,205,199,204,3000,1.25,1\n",
        "2024-01-02,\"BRK,\"\"B\",300,305,299,304,4000,300,305,299,304,4000,0,1\n"
    );
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/prices"))
        .and(query_param("format", "csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(csv))
        .expect(1)
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    assert_eq!(
        client.get_bulk_eod_prices().await.unwrap(),
        serde_json::json!({
            "prices": [
                {
                    "date": "2024-01-02",
                    "ticker": "AAPL",
                    "open": 100.0,
                    "high": 105.0,
                    "low": 99.0,
                    "close": 104.0,
                    "volume": 1000.0,
                    "adjOpen": 50.0,
                    "adjHigh": 52.5,
                    "adjLow": 49.5,
                    "adjClose": 52.0,
                    "adjVolume": 2000.0,
                    "divCash": 0.0,
                    "splitFactor": 2.0
                },
                {
                    "date": "2024-01-02",
                    "ticker": "MSFT",
                    "open": 200.0,
                    "high": 205.0,
                    "low": 199.0,
                    "close": 204.0,
                    "volume": 3000.0,
                    "adjOpen": 200.0,
                    "adjHigh": 205.0,
                    "adjLow": 199.0,
                    "adjClose": 204.0,
                    "adjVolume": 3000.0,
                    "divCash": 1.25,
                    "splitFactor": 1.0
                },
                {
                    "date": "2024-01-02",
                    "ticker": "BRK,\"B",
                    "open": 300.0,
                    "high": 305.0,
                    "low": 299.0,
                    "close": 304.0,
                    "volume": 4000.0,
                    "adjOpen": 300.0,
                    "adjHigh": 305.0,
                    "adjLow": 299.0,
                    "adjClose": 304.0,
                    "adjVolume": 4000.0,
                    "divCash": 0.0,
                    "splitFactor": 1.0
                }
            ],
            "historyRefreshTickers": ["AAPL", "MSFT"]
        })
    );
}

#[tokio::test]
async fn bulk_eod_prices_read_named_csv_headers_when_tiingo_reorders_columns() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/prices"))
        .and(query_param("format", "csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(concat!(
            "ticker,splitFactor,adjClose,date,open,high,low,close,volume,adjOpen,adjHigh,adjLow,adjVolume,divCash\n",
            "AAPL,1,99,2024-01-02,100,105,98,101,1000,98,103,96,1000,0\n"
        )))
        .expect(1)
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    let result = client.get_bulk_eod_prices().await.unwrap();

    assert_eq!(result["prices"][0]["date"], "2024-01-02");
    assert_eq!(result["prices"][0]["ticker"], "AAPL");
    assert_eq!(result["prices"][0]["close"], 101.0);
    assert_eq!(result["prices"][0]["adjClose"], 99.0);
}

#[tokio::test]
async fn bulk_eod_prices_reject_malformed_numeric_csv_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(concat!(
            "date,ticker,open,high,low,close,volume,adjOpen,adjHigh,adjLow,adjClose,adjVolume,divCash,splitFactor\n",
            "2024-01-02,AAPL,not-a-number,105,99,104,1000,100,105,99,104,1000,0,1\n"
        )))
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    let error = client.get_bulk_eod_prices().await.unwrap_err();

    assert!(matches!(error, TiingoError::Decode { .. }));
}

#[tokio::test]
async fn bulk_eod_prices_reject_csv_without_the_required_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    let error = client.get_bulk_eod_prices().await.unwrap_err();

    assert!(matches!(error, TiingoError::Decode { .. }));
}

#[tokio::test]
async fn bulk_eod_prices_reject_oversized_csv_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(8 * 1024 * 1024 + 1)))
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    let error = client.get_bulk_eod_prices().await.unwrap_err();

    assert!(matches!(error, TiingoError::ResponseTooLarge { .. }));
}

#[tokio::test]
async fn ticker_metadata_rejects_missing_empty_oversized_and_invalid_columns_locally() {
    let server = MockServer::start().await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    for columns in [
        vec![],
        vec!["".to_owned()],
        vec!["ticker".to_owned(); 33],
        vec!["not-a-ticker-metadata-column".to_owned()],
    ] {
        let error = client.get_ticker_metadata(&columns).await.unwrap_err();
        assert!(matches!(error, TiingoError::Validation(_)));
    }

    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn ticker_metadata_serializes_valid_columns_once_in_caller_order() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/meta"))
        .and(query_param("columns", "ticker,permaTicker,openfigi"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "ticker": "AAPL",
                "permaTicker": "AAPL",
                "openfigi": "BBG000B9XRY4"
            }])),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    assert_eq!(
        client
            .get_ticker_metadata(&[
                "ticker".to_owned(),
                "permaTicker".to_owned(),
                "openfigi".to_owned(),
            ])
            .await
            .unwrap(),
        serde_json::json!([{
            "ticker": "AAPL",
            "permaTicker": "AAPL",
            "openfigi": "BBG000B9XRY4"
        }])
    );
    let request = server.received_requests().await.unwrap().pop().unwrap();
    assert_eq!(
        request
            .url
            .query_pairs()
            .filter(|(name, _)| name == "columns")
            .collect::<Vec<_>>(),
        vec![("columns".into(), "ticker,permaTicker,openfigi".into())]
    );
}

#[tokio::test]
async fn ticker_metadata_preserves_vendor_supplied_route_not_found_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/meta"))
        .and(query_param("columns", "ticker"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not available"))
        .expect(1)
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    let error = client
        .get_ticker_metadata(&["ticker".to_owned()])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::NotFound { .. }));
}
