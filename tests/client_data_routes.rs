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
        .get_daily_fundamentals("AAPL", populated_range(), None)
        .await
        .unwrap();
    client.get_company_meta("AAPL,MSFT", None).await.unwrap();
    client
        .get_dividends("AAPL", populated_range())
        .await
        .unwrap();
    client
        .get_dividend_yield("AAPL", populated_range(), None)
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

#[tokio::test]
async fn column_extensions_and_corporate_action_batches_use_exact_routes_and_queries() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));

    for route in [
        "/tiingo/fundamentals/AAPL/daily",
        "/tiingo/fundamentals/meta",
        "/tiingo/corporate-actions/AAPL/distribution-yield",
        "/tiingo/corporate-actions/distributions",
        "/tiingo/corporate-actions/splits",
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response.clone())
            .expect(1)
            .mount(&server)
            .await;
    }

    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();
    client
        .get_daily_fundamentals(
            "AAPL",
            populated_range(),
            Some(&["marketCap".to_owned(), "peRatio".to_owned()]),
        )
        .await
        .unwrap();
    client
        .get_company_meta(
            "AAPL,MSFT",
            Some(&["ticker".to_owned(), "sector".to_owned()]),
        )
        .await
        .unwrap();
    client
        .get_dividend_yield(
            "AAPL",
            populated_range(),
            Some(&["trailing12MoYield".to_owned()]),
        )
        .await
        .unwrap();
    client
        .get_distributions_by_ex_date(Some(NaiveDate::from_ymd_opt(2027, 2, 15).unwrap()))
        .await
        .unwrap();
    client
        .get_splits_by_ex_date(Some(NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()))
        .await
        .unwrap();

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
                    ("columns".to_owned(), "trailing12MoYield".to_owned()),
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/corporate-actions/distributions".to_owned(),
                vec![("exDate".to_owned(), "2027-02-15".to_owned())],
            ),
            (
                "/tiingo/corporate-actions/splits".to_owned(),
                vec![("exDate".to_owned(), "2027-03-01".to_owned())],
            ),
            (
                "/tiingo/fundamentals/AAPL/daily".to_owned(),
                vec![
                    ("columns".to_owned(), "marketCap,peRatio".to_owned()),
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/fundamentals/meta".to_owned(),
                vec![
                    ("columns".to_owned(), "ticker,sector".to_owned()),
                    ("tickers".to_owned(), "AAPL,MSFT".to_owned()),
                ],
            ),
        ]
    );
}

#[tokio::test]
async fn column_extensions_reject_invalid_lists_before_requesting_tiingo() {
    let server = MockServer::start().await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    for columns in [
        vec![],
        vec!["invalid-name".to_owned()],
        vec!["marketCap".to_owned(); 33],
    ] {
        let error = client
            .get_daily_fundamentals("AAPL", DateRange::default(), Some(&columns))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            tiingo_mcp::error::TiingoError::Validation(_)
        ));
    }

    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn absent_column_and_batch_ex_date_values_omit_query_parameters() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));
    for route in [
        "/tiingo/fundamentals/AAPL/daily",
        "/tiingo/fundamentals/meta",
        "/tiingo/corporate-actions/AAPL/distribution-yield",
        "/tiingo/corporate-actions/distributions",
        "/tiingo/corporate-actions/splits",
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response.clone())
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    client
        .get_daily_fundamentals("AAPL", DateRange::default(), None)
        .await
        .unwrap();
    client.get_company_meta("AAPL", None).await.unwrap();
    client
        .get_dividend_yield("AAPL", DateRange::default(), None)
        .await
        .unwrap();
    client.get_distributions_by_ex_date(None).await.unwrap();
    client.get_splits_by_ex_date(None).await.unwrap();

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
                vec![],
            ),
            ("/tiingo/corporate-actions/distributions".to_owned(), vec![]),
            ("/tiingo/corporate-actions/splits".to_owned(), vec![]),
            ("/tiingo/fundamentals/AAPL/daily".to_owned(), vec![]),
            (
                "/tiingo/fundamentals/meta".to_owned(),
                vec![("tickers".to_owned(), "AAPL".to_owned())],
            ),
        ]
    );
}

#[tokio::test]
async fn funds_search_and_crypto_yield_clients_use_exact_routes_and_queries() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));

    for route in [
        "/tiingo/funds/VFIAX",
        "/tiingo/funds/VFIAX/metrics",
        "/tiingo/utilities/search",
        "/tiingo/crypto-yield/platforms",
        "/tiingo/crypto-yield/pools",
        "/tiingo/crypto-yield/ticks",
        "/tiingo/crypto-yield/aavev2_usdc/metrics",
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response.clone())
            .expect(1)
            .mount(&server)
            .await;
    }

    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();
    client.get_fund_metadata("VFIAX").await.unwrap();
    client.get_fund_fee_metrics("VFIAX").await.unwrap();
    client.search_tiingo_assets("  Apple  ").await.unwrap();
    client
        .get_crypto_yield_platforms(Some(&["AAVEV2".to_owned(), "COMPOUND".to_owned()]))
        .await
        .unwrap();
    client
        .get_crypto_yield_pools(
            Some(&["aavev2_usdc".to_owned(), "compound_usdc".to_owned()]),
            Some(&["AAVEV2".to_owned(), "COMPOUND".to_owned()]),
        )
        .await
        .unwrap();
    client
        .get_crypto_yield_ticks(
            Some(&["aavev2_usdc".to_owned(), "compound_usdc".to_owned()]),
            Some(&["AAVEV2".to_owned(), "COMPOUND".to_owned()]),
        )
        .await
        .unwrap();
    client
        .get_crypto_yield_metrics(
            "aavev2_usdc",
            populated_range(),
            Some(IntradayResample::FiveMinutes),
        )
        .await
        .unwrap();

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
                "/tiingo/crypto-yield/aavev2_usdc/metrics".to_owned(),
                vec![
                    ("endDate".to_owned(), "2024-01-31".to_owned()),
                    ("resampleFreq".to_owned(), "5min".to_owned()),
                    ("startDate".to_owned(), "2024-01-01".to_owned()),
                ],
            ),
            (
                "/tiingo/crypto-yield/platforms".to_owned(),
                vec![("platformCodes".to_owned(), "AAVEV2,COMPOUND".to_owned())],
            ),
            (
                "/tiingo/crypto-yield/pools".to_owned(),
                vec![
                    ("platformCodes".to_owned(), "AAVEV2,COMPOUND".to_owned()),
                    (
                        "poolCodes".to_owned(),
                        "aavev2_usdc,compound_usdc".to_owned(),
                    ),
                ],
            ),
            (
                "/tiingo/crypto-yield/ticks".to_owned(),
                vec![
                    ("platformCodes".to_owned(), "AAVEV2,COMPOUND".to_owned()),
                    (
                        "poolCodes".to_owned(),
                        "aavev2_usdc,compound_usdc".to_owned(),
                    ),
                ],
            ),
            ("/tiingo/funds/VFIAX".to_owned(), vec![]),
            ("/tiingo/funds/VFIAX/metrics".to_owned(), vec![]),
            (
                "/tiingo/utilities/search".to_owned(),
                vec![("query".to_owned(), "Apple".to_owned())],
            ),
        ]
    );
}

#[tokio::test]
async fn crypto_yield_omits_absent_filters_and_validates_safe_codes_and_date_ordering() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));
    for route in [
        "/tiingo/crypto-yield/platforms",
        "/tiingo/crypto-yield/pools",
        "/tiingo/crypto-yield/ticks",
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response.clone())
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    client.get_crypto_yield_platforms(None).await.unwrap();
    client.get_crypto_yield_pools(None, None).await.unwrap();
    client.get_crypto_yield_ticks(None, None).await.unwrap();

    for values in [
        vec![],
        vec!["aavev2/usdc".to_owned()],
        vec!["aavev2_usdc".to_owned(); 101],
    ] {
        let error = client
            .get_crypto_yield_pools(Some(&values), None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            tiingo_mcp::error::TiingoError::Validation(_)
        ));
    }
    let error = client
        .get_crypto_yield_metrics(
            "aavev2_usdc",
            DateRange {
                start_date: Some(NaiveDate::from_ymd_opt(2024, 2, 1).unwrap()),
                end_date: Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            },
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        tiingo_mcp::error::TiingoError::Validation(_)
    ));
    let error = client
        .get_crypto_yield_metrics("aavev2/usdc", DateRange::default(), None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        tiingo_mcp::error::TiingoError::Validation(_)
    ));

    for request in server.received_requests().await.unwrap() {
        assert!(request.url.query().is_none());
    }
}

#[tokio::test]
async fn search_requires_a_trimmed_nonblank_bounded_query_before_requesting_tiingo() {
    let server = MockServer::start().await;
    let client = TiingoClient::new(test_config(Url::parse(&server.uri()).unwrap())).unwrap();

    for query in ["", " \t\n ", &"x".repeat(257)] {
        let error = client.search_tiingo_assets(query).await.unwrap_err();
        assert!(matches!(
            error,
            tiingo_mcp::error::TiingoError::Validation(_)
        ));
    }

    assert!(server.received_requests().await.unwrap().is_empty());
}
