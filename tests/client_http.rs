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

#[tokio::test]
async fn empty_key_is_a_sanitized_configuration_error() {
    let client = TiingoClient::new(test_config(
        Url::parse("http://127.0.0.1:9").unwrap(),
        Some(""),
    ))
    .unwrap();

    let error = client
        .get_json("stock metadata", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Configuration(_)));
    assert!(!error.to_string().contains("Token "));
}

#[test]
fn config_debug_redacts_the_api_key() {
    let config = test_config(
        Url::parse("https://api.tiingo.com").unwrap(),
        Some("debug-secret"),
    );

    let rendered = format!("{config:?}");
    assert!(!rendered.contains("debug-secret"));
    assert!(rendered.contains("[REDACTED]"));
    assert!(rendered.contains("api.tiingo.com"));
}

#[test]
fn validation_payload_includes_the_actionable_detail() {
    let payload = TiingoError::Validation("tickers cannot be empty".into()).payload();

    assert_eq!(payload.kind, "validation");
    assert_eq!(
        payload.message,
        "tickers cannot be empty. Correct the request and try again."
    );
    assert_eq!(payload.status_code, None);
}

#[test]
fn public_error_classes_produce_actionable_payloads() {
    let cases = [
        (
            TiingoError::from_status("stock metadata", 404, String::new()),
            "not_found",
            Some(404),
            "Check the requested identifier",
        ),
        (
            TiingoError::from_status("stock prices", 429, String::new()),
            "rate_limit",
            Some(429),
            "Wait briefly",
        ),
        (
            TiingoError::from_status("stock prices", 503, String::new()),
            "transient",
            Some(503),
            "Retry the request shortly",
        ),
        (
            TiingoError::Decode {
                capability: "stock prices",
            },
            "decode",
            None,
            "unreadable response",
        ),
        (
            TiingoError::ResponseTooLarge {
                capability: "stock prices",
                limit: 1024,
            },
            "response_too_large",
            None,
            "exceeded 1024 bytes",
        ),
    ];

    for (error, kind, status_code, guidance) in cases {
        let payload = error.payload();
        assert_eq!(payload.kind, kind);
        assert_eq!(payload.status_code, status_code);
        assert!(payload.message.contains(guidance));
        assert!(payload.message.contains("stock"));
    }
}

#[test]
fn environment_factory_builds_the_default_tiingo_client() {
    assert!(TiingoClient::from_env().is_ok());
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

#[test]
fn rejects_retry_policies_that_allow_more_than_three_attempts() {
    let mut config = test_config(Url::parse("http://127.0.0.1:9").unwrap(), Some("test-key"));
    config.retry.max_attempts = 4;

    assert!(TiingoClient::new(config).is_err());
}

#[tokio::test]
async fn rejects_api_keys_that_cannot_form_an_authorization_header() {
    let client = TiingoClient::new(test_config(
        Url::parse("http://127.0.0.1:9").unwrap(),
        Some("invalid\nkey"),
    ))
    .unwrap();

    let error = client
        .get_json("stock metadata", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Configuration(_)));
    assert!(!error.to_string().contains("invalid\nkey"));
    assert!(!error.to_string().contains("Token "));
}

#[tokio::test]
async fn connection_failures_are_classified_as_transport_errors() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut config = test_config(
        Url::parse(&format!("http://{address}")).unwrap(),
        Some("test-key"),
    );
    config.retry.max_attempts = 1;
    let client = TiingoClient::new(config).unwrap();

    let error = client
        .get_json("stock metadata", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Transport { .. }));
    assert_eq!(error.payload().kind, "transport");
}

#[tokio::test]
async fn request_deadlines_are_classified_as_timeout_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(100))
                .set_body_json(serde_json::json!({"ok": true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut config = test_config(Url::parse(&server.uri()).unwrap(), Some("test-key"));
    config.request_timeout = Duration::from_millis(10);
    config.retry.max_attempts = 1;
    let client = TiingoClient::new(config).unwrap();

    let error = client
        .get_json("stock metadata", "/tiingo/daily/AAPL", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Timeout { .. }));
    assert_eq!(error.payload().kind, "timeout");
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
async fn csv_requests_share_token_authentication_query_and_retry_behavior() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/prices"))
        .and(header("authorization", "Token test-key"))
        .and(query_param("format", "csv"))
        .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/prices"))
        .and(header("authorization", "Token test-key"))
        .and(query_param("format", "csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ticker,close\nAAPL,185.92\n"))
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
            .get_csv(
                "bulk EOD prices",
                "/tiingo/daily/prices",
                &[("format", "csv".to_owned())],
            )
            .await
            .unwrap(),
        "ticker,close\nAAPL,185.92\n"
    );
}

#[tokio::test]
async fn csv_authentication_and_entitlement_errors_are_classified_and_redacted() {
    for (status, expected) in [(401, "authentication"), (403, "entitlement")] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_string("Authorization: Token test-key; key=test-key"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = TiingoClient::new(test_config(
            Url::parse(&server.uri()).unwrap(),
            Some("test-key"),
        ))
        .unwrap();

        let error = client
            .get_csv("bulk EOD prices", "/tiingo/daily/prices", &[])
            .await
            .unwrap_err();

        assert_eq!(error.payload().kind, expected);
        assert!(!error.to_string().contains("test-key"));
        assert!(!error.payload().message.contains("test-key"));
    }
}

#[tokio::test]
async fn csv_rejects_cross_origin_paths_before_contacting_another_server() {
    let configured_server = MockServer::start().await;
    let alternate_server = MockServer::start().await;
    let client = TiingoClient::new(test_config(
        Url::parse(&configured_server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    let error = client
        .get_csv(
            "bulk EOD prices",
            &format!("{}/tiingo/daily/prices", alternate_server.uri()),
            &[],
        )
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::Validation(_)));
    assert!(
        alternate_server
            .received_requests()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn csv_rejects_a_response_larger_than_eight_mebibytes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(8 * 1024 * 1024 + 1)))
        .mount(&server)
        .await;
    let client = TiingoClient::new(test_config(
        Url::parse(&server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    let error = client
        .get_csv("bulk EOD prices", "/tiingo/daily/prices", &[])
        .await
        .unwrap_err();

    assert!(matches!(error, TiingoError::ResponseTooLarge { .. }));
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
async fn rejects_cross_origin_paths_before_contacting_another_server() {
    let configured_server = MockServer::start().await;
    let alternate_server = MockServer::start().await;
    let client = TiingoClient::new(test_config(
        Url::parse(&configured_server.uri()).unwrap(),
        Some("test-key"),
    ))
    .unwrap();

    let absolute = client
        .get_json(
            "stock prices",
            &format!("{}/tiingo/daily/AAPL", alternate_server.uri()),
            &[],
        )
        .await
        .unwrap_err();
    let network_path = client
        .get_json(
            "stock prices",
            &format!("//{}/tiingo/daily/AAPL", alternate_server.address()),
            &[],
        )
        .await
        .unwrap_err();

    assert!(matches!(absolute, TiingoError::Validation(_)));
    assert!(matches!(network_path, TiingoError::Validation(_)));
    assert!(
        alternate_server
            .received_requests()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn upstream_error_payload_and_display_never_expose_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_string("key=test-key\nAuthorization: Token test-key"),
        )
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
    let payload = error.payload();

    assert!(!error.to_string().contains("test-key"));
    assert!(!error.to_string().contains("Token test-key"));
    assert!(!payload.message.contains("test-key"));
    assert!(!payload.message.contains("Token test-key"));
    assert!(!payload.message.contains("Authorization:"));
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
