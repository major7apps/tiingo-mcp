use std::process::Command;

use futures_util::{SinkExt, StreamExt};
use tiingo_mcp::websocket::{
    protocol::Service,
    registry::{MarketDataRegistry, StartRequest, TiingoConnector},
};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing_subscriber::EnvFilter;

const CHILD_ENV: &str = "TIINGO_WEBSOCKET_TRACE_CHILD";
const ENDPOINT_ENV: &str = "TIINGO_WEBSOCKET_TRACE_ENDPOINT";
const API_KEY: &str = "trace-api-key-must-not-leak";
const UPSTREAM_ID: &str = "trace-upstream-id-must-not-leak";

#[test]
fn tungstenite_trace_never_emits_credentials_or_upstream_ids() {
    if std::env::var_os(CHILD_ENV).is_some() {
        run_trace_child();
        return;
    }

    let output = tokio::runtime::Runtime::new()
        .expect("build parent runtime")
        .block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock WebSocket");
            let endpoint = format!("ws://{}", listener.local_addr().expect("mock address"));
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept mock client");
                let mut socket = accept_async(stream).await.expect("accept WebSocket");
                socket
                    .next()
                    .await
                    .expect("receive subscribe frame")
                    .expect("valid subscribe frame");
                socket
                    .send(Message::text(
                        serde_json::json!({
                            "messageType": "I",
                            "data": {"subscriptionId": UPSTREAM_ID},
                            "response": {"code": 200, "message": "subscribed"}
                        })
                        .to_string(),
                    ))
                    .await
                    .expect("send subscription acknowledgement");
                socket
                    .next()
                    .await
                    .expect("receive unsubscribe frame")
                    .expect("valid unsubscribe frame");
            });
            let output = tokio::task::spawn_blocking(move || {
                Command::new(std::env::current_exe().expect("locate integration-test process"))
                    .args([
                        "--exact",
                        "tungstenite_trace_never_emits_credentials_or_upstream_ids",
                        "--nocapture",
                    ])
                    .env(CHILD_ENV, "1")
                    .env(ENDPOINT_ENV, endpoint)
                    .env("RUST_LOG", "tungstenite=trace")
                    .output()
                    .expect("run isolated trace child")
            })
            .await
            .expect("trace child task joined");
            server.await.expect("mock server joined");
            output
        });
    assert!(
        output.status.success(),
        "trace child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let mut captured = output.stdout;
    captured.extend(output.stderr);
    let captured = String::from_utf8_lossy(&captured);
    assert!(!captured.contains(API_KEY), "API key escaped trace output");
    assert!(
        !captured.contains(UPSTREAM_ID),
        "upstream subscription ID escaped trace output"
    );
}

fn run_trace_child() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
        .expect("initialize isolated tracing subscriber");

    tokio::runtime::Runtime::new()
        .expect("build child runtime")
        .block_on(async {
            let endpoint = std::env::var(ENDPOINT_ENV).expect("parent mock endpoint");
            let connector =
                TiingoConnector::with_endpoints(endpoint.clone(), endpoint).expect("connector");
            let registry = MarketDataRegistry::with_connector(Some(API_KEY.into()), connector);
            let started = registry
                .start(StartRequest {
                    service: Service::Iex,
                    symbols: vec!["AAPL".into()],
                    threshold_level: None,
                    confirm_iex_market_data_agreement: false,
                })
                .await
                .expect("start mock subscription");
            registry
                .stop(&started.id)
                .await
                .expect("stop mock subscription");
        });
}
