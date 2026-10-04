#[allow(dead_code)]
#[path = "support/live_cases.rs"]
mod live_cases;

use std::{ffi::OsString, time::Duration};

use anyhow::Context;
use live_cases::{REST_CASES, RestCase};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    service::RunningService,
    transport::TokioChildProcess,
};
use serde_json::Value;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Select the installed test executable when supplied, otherwise use the Cargo-built binary.
fn test_binary() -> OsString {
    std::env::var_os("TIINGO_MCP_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_tiingo-mcp").into())
}

/// Initialize the actual stdio binary with explicit live-key checks or credential-free test isolation.
async fn connect(live: bool) -> anyhow::Result<RunningService<RoleClient, ()>> {
    let mut command = tokio::process::Command::new(test_binary());
    command.kill_on_drop(true);
    if live {
        anyhow::ensure!(
            std::env::var("TIINGO_API_KEY").is_ok_and(|key| !key.trim().is_empty()),
            "TIINGO_API_KEY must be nonempty for explicitly selected live tests"
        );
    } else {
        command.env_remove("TIINGO_API_KEY");
    }
    tokio::time::timeout(CALL_TIMEOUT, ().serve(TokioChildProcess::new(command)?))
        .await
        .context("stdio initialization timed out")?
        .map_err(|_| anyhow::anyhow!("stdio initialization failed"))
}

/// Call one tool with an object argument and enforce the ten-second live-harness deadline.
async fn call(
    client: &RunningService<RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<CallToolResult> {
    let arguments = arguments
        .as_object()
        .context("tool arguments must be an object")?
        .clone();
    tokio::time::timeout(
        CALL_TIMEOUT,
        client.call_tool(CallToolRequestParams::new(tool).with_arguments(arguments)),
    )
    .await
    .context("bounded stdio tool call timed out")?
    .map_err(|_| anyhow::anyhow!("stdio tool request failed"))
}

#[derive(Debug, PartialEq, Eq)]
enum CaseOutcome {
    Validated,
    Entitlement,
    NoData,
}

/// Require the success marker, source attribution, and JSON-text/structured-result parity.
fn data(result: &CallToolResult) -> anyhow::Result<&Value> {
    let structured = result
        .structured_content
        .as_ref()
        .context("missing structured content")?;
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .context("missing JSON text")?;
    let text: Value = serde_json::from_str(&text.text).context("invalid JSON text")?;
    if result.is_error == Some(true) {
        anyhow::ensure!(
            text == structured["error"],
            "MCP error text/structured parity failed"
        );
        anyhow::bail!("MCP returned a classified error");
    }
    anyhow::ensure!(
        result.is_error == Some(false),
        "MCP success isError flag was missing"
    );
    anyhow::ensure!(
        structured["meta"]["source"] == "tiingo",
        "MCP source marker was missing"
    );
    anyhow::ensure!(
        text == structured["data"],
        "MCP success text/structured parity failed"
    );
    Ok(&structured["data"])
}

/// Distinguish validated REST data, an empty result, and a classified entitlement rejection.
fn classify_case(
    case: &RestCase,
    result: &CallToolResult,
    arguments: &Value,
) -> anyhow::Result<CaseOutcome> {
    if result.is_error == Some(true) {
        if is_entitlement(result)? {
            return Ok(CaseOutcome::Entitlement);
        }
        anyhow::bail!("MCP returned a non-entitlement error");
    }
    let data = data(result)?;
    if data.as_array().is_some_and(Vec::is_empty) {
        return Ok(CaseOutcome::NoData);
    }
    anyhow::ensure!(
        case.validate(data, arguments),
        "wrong-family or invalid live response shape"
    );
    Ok(CaseOutcome::Validated)
}

/// Require MCP error parity and an explicit HTTP 403 entitlement classification.
fn is_entitlement(result: &CallToolResult) -> anyhow::Result<bool> {
    let structured = result
        .structured_content
        .as_ref()
        .context("missing structured error")?;
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .context("missing error text")?;
    let text: Value = serde_json::from_str(&text.text)?;
    anyhow::ensure!(
        result.is_error == Some(true) && text == structured["error"],
        "MCP error flag or text/structured parity failed"
    );
    Ok(structured["error"]["kind"] == "entitlement" && structured["error"]["status_code"] == 403)
}

#[derive(Debug, PartialEq, Eq)]
enum SubscriptionPollOutcome {
    Active,
    Entitlement,
}

/// Require the local subscription identity and classify active data versus terminal entitlement.
fn classify_subscription_poll(
    result: &CallToolResult,
    subscription_id: &str,
) -> anyhow::Result<SubscriptionPollOutcome> {
    let polled = data(result)?;
    anyhow::ensure!(
        polled["id"] == subscription_id,
        "poll changed local subscription identity"
    );
    let state = polled["state"].as_str().context("poll state missing")?;
    let terminal = polled.get("terminalError");
    if state == "failed" && terminal.is_some_and(|value| value == "entitlement") {
        return Ok(SubscriptionPollOutcome::Entitlement);
    }
    anyhow::ensure!(
        state == "active" && terminal.is_none(),
        "poll returned a non-active or terminal state"
    );
    Ok(SubscriptionPollOutcome::Active)
}

/// Send EOF and await bounded child cleanup before the process kill fallback.
async fn close(mut client: RunningService<RoleClient, ()>) -> anyhow::Result<()> {
    // Closing the child transport sends EOF and waits for cleanup before its kill fallback.
    tokio::time::timeout(CALL_TIMEOUT, client.close())
        .await
        .context("stdio child cleanup timed out")?
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("stdio child cleanup failed"))
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY; 32 bounded REST calls, at most 96 HTTP attempts"]
async fn live_mcp_bounded_rest_end_to_end() -> anyhow::Result<()> {
    let client = connect(true).await?;
    let mut validated = 0;
    let mut entitlement = 0;
    let mut no_data = 0;
    let mut failures = 0;
    for case in REST_CASES {
        let arguments = case.arguments();
        let outcome = match call(&client, case.tool, arguments.clone()).await {
            Ok(result) => classify_case(case, &result, &arguments),
            Err(error) => Err(error),
        };
        match outcome {
            Ok(CaseOutcome::Validated) => {
                validated += 1;
                println!(
                    "LIVE_STDIO {}: validated shape and text/structured parity",
                    case.tool
                );
            }
            Ok(CaseOutcome::Entitlement) => {
                entitlement += 1;
                println!(
                    "LIVE_STDIO {}: entitlement HTTP 403; success not established",
                    case.tool
                );
            }
            Ok(CaseOutcome::NoData) => {
                no_data += 1;
                println!("LIVE_STDIO {}: no data; success not established", case.tool);
            }
            Err(_) => {
                failures += 1;
                println!(
                    "LIVE_STDIO {}: failed validation or bounded call",
                    case.tool
                );
            }
        }
    }
    let cleanup = close(client).await;
    println!(
        "LIVE_STDIO REST: logical_calls={}, max_http_attempts={}, validated={validated}, entitlement={entitlement}, no_data={no_data}, failures={failures}, call_timeout={CALL_TIMEOUT:?}",
        REST_CASES.len(),
        REST_CASES.len() * 3
    );
    cleanup?;
    anyhow::ensure!(
        failures == 0,
        "one or more stdio REST cases failed; see sanitized classifications"
    );
    Ok(())
}

/// Run one bounded level-six start/update/poll/stop lifecycle and clean up after failure too.
async fn live_subscription_update(service: &str) -> anyhow::Result<()> {
    let client = connect(true).await?;
    let mut subscription_id = None;
    let outcome = async {
        let started = call(&client, "start_market_data_subscription", serde_json::json!({
            "service": service, "symbols": ["AAPL"], "threshold_level": 6
        })).await?;
        if started.is_error == Some(true) {
            if is_entitlement(&started)? {
                println!("LIVE_STDIO {service}: start entitlement; update/poll/stop skipped because no subscription exists");
                return Ok(());
            }
            anyhow::bail!("stdio subscription start failed");
        }
        let started = data(&started)?;
        let id = started["id"].as_str().context("start omitted local subscription id")?.to_owned();
        subscription_id = Some(id.clone());
        if started["state"] != "active" {
            let polled = call(&client, "poll_market_data_subscription", serde_json::json!({
                "subscription_id": id, "limit": 1, "max_wait_ms": 0
            })).await?;
            anyhow::ensure!(classify_subscription_poll(&polled, &id)? == SubscriptionPollOutcome::Entitlement,
                "non-active start did not produce terminal entitlement evidence");
            println!("LIVE_STDIO {service}: entitlement before start publication; update skipped, cleanup still required");
            return Ok(());
        }
        let updated = call(&client, "update_market_data_subscription", serde_json::json!({
            "subscription_id": id, "remove_symbols": ["AAPL"], "add_symbols": ["MSFT"]
        })).await?;
        if updated.is_error == Some(true) && is_entitlement(&updated)? {
            println!("LIVE_STDIO {service}: update entitlement HTTP 403; poll skipped, cleanup still required");
            return Ok(());
        }
        let updated = data(&updated)?;
        anyhow::ensure!(updated["id"] == id && updated["state"] == "active" && updated["symbols"] == serde_json::json!(["MSFT"]), "update identity/state/symbols were inconsistent");
        let polled = call(&client, "poll_market_data_subscription", serde_json::json!({
            "subscription_id": id, "limit": 1, "max_wait_ms": 1000
        })).await?;
        if classify_subscription_poll(&polled, &id)? == SubscriptionPollOutcome::Entitlement {
            println!("LIVE_STDIO {service}: entitlement after activation; delivery not established");
            return Ok(());
        }
        let polled = data(&polled)?;
        let events = polled["events"].as_array().context("poll events missing")?;
        anyhow::ensure!(events.len() <= 1, "poll exceeded requested event bound");
        let protocol_service = if service == "iex" {
            tiingo_mcp::websocket::protocol::Service::Iex
        } else {
            tiingo_mcp::websocket::protocol::Service::Consolidated
        };
        let mut delivered = false;
        for event in events {
            // Cursor zero may replay valid AAPL events queued before the update acknowledgement.
            delivered |= live_cases::validate_market_event(&event["payload"], protocol_service, &["AAPL", "MSFT"])?;
        }
        println!("LIVE_STDIO {service}: update acknowledged, active poll, market_data_delivered={delivered}");
        Ok(())
    }.await;
    let stop = if let Some(id) = subscription_id {
        match call(
            &client,
            "stop_market_data_subscription",
            serde_json::json!({"subscription_id": id}),
        )
        .await
        {
            Ok(result) => data(&result).and_then(|value| {
                anyhow::ensure!(value["state"] == "stopped", "stop did not finish cleanup");
                Ok(())
            }),
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    };
    let cleanup = close(client).await;
    stop?;
    cleanup?;
    outcome
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY; one level-6 IEX start/update/poll/stop lifecycle"]
async fn live_mcp_iex_subscription_update_end_to_end() -> anyhow::Result<()> {
    live_subscription_update("iex").await
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY and consolidated session; one level-6 start/update/poll/stop lifecycle"]
async fn live_mcp_consolidated_subscription_update_end_to_end() -> anyhow::Result<()> {
    live_subscription_update("consolidated").await
}

#[tokio::test]
async fn stdio_live_harness_closes_after_configuration_failure() -> anyhow::Result<()> {
    let client = connect(false).await?;
    let result = call(
        &client,
        "get_stock_metadata",
        serde_json::json!({"ticker": "AAPL"}),
    )
    .await;
    let cleanup = close(client).await;
    let result = result?;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content.as_ref().unwrap()["error"]["kind"],
        "configuration"
    );
    assert!(classify_case(&REST_CASES[0], &result, &REST_CASES[0].arguments()).is_err());
    cleanup
}

#[test]
fn bounded_rest_cases_are_unique_filtered_and_parseable() {
    let names = REST_CASES
        .iter()
        .map(|case| case.tool)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names.len(), 32);
    assert_eq!(names.len(), REST_CASES.len());
    for case in REST_CASES {
        let arguments = case.arguments();
        assert!(arguments.is_object());
        match case.tool {
            "get_iex_market_snapshot" => {
                assert_eq!(arguments["tickers"], serde_json::json!(["AAPL"]))
            }
            "get_equity_realtime_snapshot" | "get_boats_snapshot" => {
                assert_eq!(arguments["ticker"], "AAPL")
            }
            "get_crypto_quote" | "get_crypto_metadata" => {
                assert_eq!(arguments["tickers"], "btcusd")
            }
            "get_crypto_yield_platforms" => {
                assert_eq!(arguments["platform_codes"], serde_json::json!(["AAVEV2"]))
            }
            "get_crypto_yield_pools" | "get_crypto_yield_ticks" => {
                assert_eq!(arguments["pool_codes"], serde_json::json!(["aavev2_usdc"]))
            }
            _ => {}
        }
    }
}

#[test]
fn live_market_delivery_requires_subscribed_identity_valid_timestamp_and_numeric_price() {
    use tiingo_mcp::websocket::protocol::Service;
    let mut payload = serde_json::json!({"messageType":"A", "service":"iex",
        "data":["2024-01-02T14:30:00Z", "AAPL", 185.92]});
    assert!(live_cases::validate_market_event(&payload, Service::Iex, &["AAPL"]).unwrap());
    assert!(live_cases::validate_market_event(&payload, Service::Consolidated, &["AAPL"]).is_err());
    payload["data"][1] = "UNSUBSCRIBED".into();
    assert!(live_cases::validate_market_event(&payload, Service::Iex, &["AAPL", "MSFT"]).is_err());
    payload["data"][1] = "AAPL".into();
    payload["data"][0] = "not-a-timestamp".into();
    assert!(live_cases::validate_market_event(&payload, Service::Iex, &["AAPL"]).is_err());
    payload["data"][0] = "2024-01-02T14:30:00Z".into();
    payload["data"][2] = "not-numeric".into();
    assert!(live_cases::validate_market_event(&payload, Service::Iex, &["AAPL"]).is_err());
    let heartbeat = serde_json::json!({"messageType":"H","response":{"code":200,"message":"ok"}});
    assert!(!live_cases::validate_market_event(&heartbeat, Service::Iex, &["AAPL"]).unwrap());
}

#[test]
fn historical_live_cases_require_requested_dates_and_ohlc_invariants() {
    let eod = REST_CASES
        .iter()
        .find(|case| case.tool == "get_stock_prices")
        .unwrap();
    let arguments = eod.arguments();
    let mut bar = serde_json::json!([{"date":"2024-01-02T00:00:00Z","open":100.0,"high":105.0,"low":99.0,"close":104.0,"volume":1000}]);
    assert!(eod.validate(&bar, &arguments));
    bar[0]["date"] = "2024-01-03T00:00:00Z".into();
    assert!(!eod.validate(&bar, &arguments));
    bar[0]["date"] = "2024-01-02T00:00:00Z".into();
    bar[0]["high"] = 90.0.into();
    assert!(!eod.validate(&bar, &arguments));
    let crypto = REST_CASES
        .iter()
        .find(|case| case.tool == "get_crypto_prices")
        .unwrap();
    assert!(!crypto.validate(
        &serde_json::json!([{"ticker":"btcusd","lastPrice":1.0}]),
        &crypto.arguments()
    ));
    let metadata = REST_CASES
        .iter()
        .find(|case| case.tool == "get_stock_metadata")
        .unwrap();
    assert!(!metadata.validate(
        &serde_json::json!({"ticker":"MSFT","name":"Microsoft"}),
        &metadata.arguments()
    ));
    let yield_case = REST_CASES
        .iter()
        .find(|case| case.tool == "get_dividend_yield")
        .unwrap();
    assert!(yield_case.validate(
        &serde_json::json!([{"date":"2024-01-02T00:00:00Z","trailingDiv1Y":0.0053871282}]),
        &yield_case.arguments()
    ));
    assert!(!yield_case.validate(
        &serde_json::json!([{"date":"2024-01-02T00:00:00Z","trailing12MoYield":0.0053871282}]),
        &yield_case.arguments()
    ));
}

#[test]
fn stdio_case_classifier_separates_entitlement_empty_data_and_corrupted_parity() {
    fn result(payload: Value, is_error: bool) -> CallToolResult {
        serde_json::from_value(serde_json::json!({
            "content":[{"type":"text","text":payload.to_string()}],
            "structuredContent": if is_error {serde_json::json!({"error":payload})}
                else {serde_json::json!({"data":payload,"meta":{"source":"tiingo"}})},
            "isError":is_error
        }))
        .unwrap()
    }
    let case = REST_CASES
        .iter()
        .find(|case| case.tool == "get_stock_prices")
        .unwrap();
    assert_eq!(
        classify_case(
            case,
            &result(serde_json::json!([]), false),
            &case.arguments()
        )
        .unwrap(),
        CaseOutcome::NoData
    );
    let entitlement = result(
        serde_json::json!({"kind":"entitlement","status_code":403,"message":"not entitled"}),
        true,
    );
    assert_eq!(
        classify_case(case, &entitlement, &case.arguments()).unwrap(),
        CaseOutcome::Entitlement
    );
    let mut corrupted = entitlement;
    corrupted.structured_content.as_mut().unwrap()["error"]["kind"] = "authentication".into();
    assert!(classify_case(case, &corrupted, &case.arguments()).is_err());
    let auth = result(
        serde_json::json!({"kind":"authentication","status_code":401,"message":"invalid key"}),
        true,
    );
    assert!(classify_case(case, &auth, &case.arguments()).is_err());
}

#[test]
fn official_corporate_dates_and_lowercase_tickers_validate_without_relaxing_code_identity() {
    let distributions = REST_CASES
        .iter()
        .find(|case| case.tool == "get_distributions_by_ex_date")
        .unwrap();
    let mut rows = serde_json::json!([{"ticker":"aapl","exDate":"2024-02-09T05:00:00.000Z","distribution":0.24}]);
    assert!(distributions.validate(&rows, &distributions.arguments()));
    rows[0]["exDate"] = "2024-02-10T05:00:00.000Z".into();
    assert!(!distributions.validate(&rows, &distributions.arguments()));
    rows[0]["exDate"] = "2024-02-09invalid".into();
    assert!(!distributions.validate(&rows, &distributions.arguments()));
    let metadata = REST_CASES
        .iter()
        .find(|case| case.tool == "get_stock_metadata")
        .unwrap();
    assert!(metadata.validate(
        &serde_json::json!({"ticker":"aapl","name":"Apple"}),
        &metadata.arguments()
    ));
    let platform = REST_CASES
        .iter()
        .find(|case| case.tool == "get_crypto_yield_platforms")
        .unwrap();
    assert!(!platform.validate(
        &serde_json::json!([{"platformCode":"aavev2","name":"Aave","network":"ETH"}]),
        &platform.arguments()
    ));
    let definitions = REST_CASES
        .iter()
        .find(|case| case.tool == "get_fundamentals_definitions")
        .unwrap();
    assert!(!definitions.validate(
        &serde_json::json!([{"ticker":"AAPL","name":"Apple"}]),
        &definitions.arguments()
    ));
    assert!(definitions.validate(
        &serde_json::json!([{"dataCode":"revenue","description":""}]),
        &definitions.arguments()
    ));
}

#[test]
fn failed_start_entitlement_uses_the_same_mcp_poll_evidence_as_post_activation_failure() {
    fn poll(state: &str, terminal: Option<&str>) -> CallToolResult {
        let mut payload = serde_json::json!({"id":"local-test-id","state":state,"events":[]});
        if let Some(terminal) = terminal {
            payload["terminalError"] = terminal.into();
        }
        serde_json::from_value(serde_json::json!({
            "content":[{"type":"text","text":payload.to_string()}],
            "structuredContent":{"data":payload,"meta":{"source":"tiingo"}},
            "isError":false
        }))
        .unwrap()
    }
    let failed = poll("failed", Some("entitlement"));
    assert_eq!(
        classify_subscription_poll(&failed, "local-test-id").unwrap(),
        SubscriptionPollOutcome::Entitlement
    );
    assert_eq!(
        classify_subscription_poll(&poll("active", None), "local-test-id").unwrap(),
        SubscriptionPollOutcome::Active
    );
    assert!(classify_subscription_poll(&failed, "another-local-id").is_err());
    for terminal in ["authentication", "protocol", "transport"] {
        assert!(
            classify_subscription_poll(&poll("failed", Some(terminal)), "local-test-id").is_err()
        );
    }
    assert!(classify_subscription_poll(&poll("reconnecting", None), "local-test-id").is_err());
    let mut corrupted = failed;
    corrupted.structured_content.as_mut().unwrap()["data"]["terminalError"] = "transport".into();
    assert!(classify_subscription_poll(&corrupted, "local-test-id").is_err());
}
