use std::time::Duration;

use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    error::TiingoError,
};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

fn test_config(base_url: Url, api_key: Option<&str>) -> Config {
    Config {
        api_key: api_key.map(str::to_owned),
        base_url,
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    }
}

#[tokio::test]
async fn missing_key_is_a_sanitized_configuration_error() {
    let client =
        TiingoClient::new(test_config(Url::parse("http://127.0.0.1:9").unwrap(), None)).unwrap();

    let error = client
        .get_json("stock metadata", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Configuration(_)));
    assert!(!error.to_string().contains("Token "));
}

#[test]
fn only_safe_transient_statuses_retry() {
    assert!(TiingoError::status_is_retryable(429));
    assert!(TiingoError::status_is_retryable(502));
    assert!(TiingoError::status_is_retryable(503));
    assert!(TiingoError::status_is_retryable(504));
    for status in [400, 401, 403, 404, 500] {
        assert!(!TiingoError::status_is_retryable(status));
    }
}

#[tokio::test]
async fn sends_token_authentication_and_only_supplied_query_values() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL"))
        .and(header("authorization", "Token test-key"))
        .and(query_param("resampleFreq", "daily"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let client = TiingoClient::new(test_config(
        Url::parse(&server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    let response = client
        .get_json(
            "stock prices",
            "/tiingo/daily/AAPL",
            &[("resampleFreq", "daily".to_owned())],
        )
        .await
        .unwrap();

    assert_eq!(response, serde_json::json!({"ok": true}));
}

#[tokio::test]
async fn retries_503_up_to_a_third_successful_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let client = TiingoClient::new(test_config(
        Url::parse(&server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    assert_eq!(
        client
            .get_json("stock prices", "/tiingo/daily/AAPL", &[])
            .await
            .unwrap(),
        serde_json::json!({"ok": true})
    );
}

#[tokio::test]
async fn does_not_retry_authentication_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad credential"))
        .expect(1)
        .mount(&server)
        .await;

    let client = TiingoClient::new(test_config(
        Url::parse(&server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    let error = client
        .get_json("stock prices", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Authentication { .. }));
}

#[tokio::test]
async fn rejects_a_response_larger_than_eight_mebibytes_before_json_decoding() {
    let server = MockServer::start().await;
    let oversized_body = "x".repeat(8 * 1024 * 1024 + 1);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(oversized_body))
        .mount(&server)
        .await;

    let client = TiingoClient::new(test_config(
        Url::parse(&server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    let error = client
        .get_json("stock prices", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::ResponseTooLarge { .. }));
}
