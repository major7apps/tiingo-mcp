use std::time::Duration;

use chrono::NaiveDate;
use tiingo_mcp::{
    client::{
        TiingoClient,
        query::{DateRange, IntradayResample, NewsQuery, NewsSort},
    },
    config::{Config, RetryPolicy},
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
async fn data_client_methods_use_the_exact_tiingo_routes_and_queries() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));

    Mock::given(method("GET"))
        .and(path("/tiingo/crypto/prices"))
        .and(query_param("tickers", "btcusd"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/crypto/prices"))
        .and(query_param("tickers", "btcusd,ethusd"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("resampleFreq", "1hour"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/crypto"))
        .and(query_param("tickers", "btcusd"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/news"))
        .and(query_param("tickers", "AAPL,MSFT"))
        .and(query_param("tags", "earnings"))
        .and(query_param("source", "reuters"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .and(query_param("limit", "25"))
        .and(query_param("offset", "50"))
        .and(query_param("sortBy", "publishedDate"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/fundamentals/definitions"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/fundamentals/AAPL/statements"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/fundamentals/AAPL/daily"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/fundamentals/meta"))
        .and(query_param("tickers", "AAPL,MSFT"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/corporate-actions/AAPL/distributions"))
        .and(query_param("startExDate", "2024-01-01"))
        .and(query_param("endExDate", "2024-01-31"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/corporate-actions/AAPL/distribution-yield"))
        .and(query_param("startDate", "2024-01-01"))
        .and(query_param("endDate", "2024-01-31"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/corporate-actions/AAPL/splits"))
        .and(query_param("startExDate", "2024-01-01"))
        .and(query_param("endExDate", "2024-01-31"))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;

    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();
    let news = NewsQuery {
        tickers: Some("AAPL,MSFT".to_owned()),
        tags: Some("earnings".to_owned()),
        source: Some("reuters".to_owned()),
        start_date: populated_range().start_date,
        end_date: populated_range().end_date,
        limit: Some(25),
        offset: Some(50),
        sort_by: Some(NewsSort::PublishedDate),
    };

    client.get_crypto_quote(Some("btcusd")).await.unwrap();
    client
        .get_crypto_prices(
            "btcusd,ethusd",
            populated_range(),
            Some(IntradayResample::OneHour),
        )
        .await
        .unwrap();
    client.get_crypto_metadata(Some("btcusd")).await.unwrap();
    client.get_news(news).await.unwrap();
    client.get_fundamentals_definitions().await.unwrap();
    client
        .get_financial_statements("AAPL", populated_range())
        .await
        .unwrap();
    client
        .get_daily_fundamentals("AAPL", populated_range())
        .await
        .unwrap();
    client.get_company_meta("AAPL,MSFT").await.unwrap();
    client
        .get_dividends("AAPL", populated_range())
        .await
        .unwrap();
    client
        .get_dividend_yield("AAPL", populated_range())
        .await
        .unwrap();
    client.get_splits("AAPL", populated_range()).await.unwrap();

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
                "/tiingo/corporate-actions/AAPL/distribution-yield".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/corporate-actions/AAPL/distributions".to_owned(),
                vec![
                    ("endExDate".to_owned(), "2024-01-31".to_owned()),
                    ("startExDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/corporate-actions/AAPL/splits".to_owned(),
                vec![
                    ("endExDate".to_owned(), "2024-01-31".to_owned()),
                    ("startExDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/crypto".to_owned(),
                vec![("tickers".to_owned(), "btcusd".to_owned())],
            ),
            (
                "/tiingo/crypto/prices".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("resampleFreq".to_owned(), "1hour".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                    ("tickers".to_owned(), "btcusd,ethusd".to_owned()),
                ],
            ),
            (
                "/tiingo/crypto/prices".to_owned(),
                vec![("tickers".to_owned(), "btcusd".to_owned())],
            ),
            (
                "/tiingo/fundamentals/AAPL/daily".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/fundamentals/AAPL/statements".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            ("/tiingo/fundamentals/definitions".to_owned(), vec![]),
            (
                "/tiingo/fundamentals/meta".to_owned(),
                vec![("tickers".to_owned(), "AAPL,MSFT".to_owned())],
            ),
            (
                "/tiingo/news".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("limit".to_owned(), "25".to_owned()),
                    ("offset".to_owned(), "50".to_owned()),
                    ("sortBy".to_owned(), "publishedDate".to_owned()),
                    ("source".to_owned(), "reuters".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                    ("tags".to_owned(), "earnings".to_owned()),
                    ("tickers".to_owned(), "AAPL,MSFT".to_owned()),
                ],
            ),
        ]
    );
}

#[tokio::test]
async fn omits_absent_optional_data_query_values() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));

    for route in ["/tiingo/crypto/prices", "/tiingo/crypto", "/tiingo/news"] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response.clone())
            .expect(1)
            .mount(&server)
            .await;
    }

    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();
    client.get_crypto_quote(None).await.unwrap();
    client.get_crypto_metadata(Some("")).await.unwrap();
    client.get_news(NewsQuery::default()).await.unwrap();

    for request in server.received_requests().await.unwrap() {
        assert!(request.url.query().is_none());
    }
}
