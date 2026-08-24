use std::time::Duration;

use chrono::NaiveDate;
use tiingo_mcp::{
    client::{
        TiingoClient,
        query::{DateRange, EodResample, IntradayResample},
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
            .get_intraday_prices(
                "AAPL",
                populated_range(),
                Some(IntradayResample::FiveMinutes),
            )
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
