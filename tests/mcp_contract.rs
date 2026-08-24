use std::{collections::BTreeSet, time::Duration};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, GetPromptRequestParams, JsonObject, ReadResourceRequestParams},
    transport::TokioChildProcess,
};
use serde_json::Value;
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
};
use tokio::process::Command;
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const PYTHON_CONTRACT: &str = include_str!("contract/baseline/python-mcp.json");

fn child_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tiingo-mcp"));
    command.env_remove("TIINGO_API_KEY");
    command
}

fn values<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value[field].as_array().unwrap()
}

fn strings(items: &[Value], field: &str) -> BTreeSet<String> {
    items
        .iter()
        .map(|item| item[field].as_str().unwrap().to_owned())
        .collect()
}

fn named<'a>(items: &'a [Value], field: &str, name: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item[field] == name)
        .unwrap_or_else(|| panic!("missing {field}={name}"))
}

fn schema_contract(schema: &Value) -> (BTreeSet<String>, BTreeSet<String>, Vec<(String, Value)>) {
    let properties = schema["properties"].as_object().unwrap();
    let property_names = properties.keys().cloned().collect();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|field| field.as_str().unwrap().to_owned())
        .collect();
    let defaults = properties
        .iter()
        .filter_map(|(name, property)| {
            property
                .get("default")
                .map(|default| (name.clone(), default.clone()))
        })
        .collect();
    (property_names, required, defaults)
}

fn prompt_arguments(prompt: &Value) -> BTreeSet<(String, bool)> {
    prompt
        .get("arguments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|argument| {
            (
                argument["name"].as_str().unwrap().to_owned(),
                argument
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        })
        .collect()
}

fn arguments(value: Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

fn baseline_resource_json(baseline: &Value, uri: &str) -> Value {
    let text = baseline["resource_contents"][uri][0]["text"]
        .as_str()
        .unwrap();
    serde_json::from_str(text).unwrap()
}

fn result_text(value: &Value) -> &str {
    value["messages"][0]["content"]["text"].as_str().unwrap()
}

#[tokio::test]
async fn child_process_exposes_the_complete_contract() -> anyhow::Result<()> {
    let transport = TokioChildProcess::new(child_command())?;
    let client = ().serve(transport).await?;

    assert_eq!(client.list_all_tools().await?.len(), 17);
    assert_eq!(client.list_all_resources().await?.len(), 3);
    assert_eq!(client.list_all_resource_templates().await?.len(), 1);
    assert_eq!(client.list_all_prompts().await?.len(), 5);

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn canonical_discovery_matches_the_frozen_python_contract() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT)?;
    let transport = TokioChildProcess::new(child_command())?;
    let client = ().serve(transport).await?;

    let rust_tools = serde_json::to_value(client.list_all_tools().await?)?;
    let rust_tools = rust_tools.as_array().unwrap();
    let python_tools = values(&baseline, "tools");
    assert_eq!(strings(rust_tools, "name"), strings(python_tools, "name"));
    for python_tool in python_tools {
        let name = python_tool["name"].as_str().unwrap();
        let rust_tool = named(rust_tools, "name", name);
        assert_eq!(
            schema_contract(&rust_tool["inputSchema"]),
            schema_contract(&python_tool["inputSchema"]),
            "{name} argument contract drifted"
        );
        assert_eq!(rust_tool["inputSchema"]["type"], "object");
        assert_eq!(rust_tool["inputSchema"]["additionalProperties"], false);
    }

    let rust_resources = serde_json::to_value(client.list_all_resources().await?)?;
    let rust_resources = rust_resources.as_array().unwrap();
    let python_resources = values(&baseline, "resources");
    assert_eq!(
        strings(rust_resources, "uri"),
        strings(python_resources, "uri")
    );

    let rust_templates = serde_json::to_value(client.list_all_resource_templates().await?)?;
    let rust_templates = rust_templates.as_array().unwrap();
    let python_templates = values(&baseline, "resource_templates");
    assert_eq!(
        strings(rust_templates, "uriTemplate"),
        strings(python_templates, "uriTemplate")
    );

    let rust_prompts = serde_json::to_value(client.list_all_prompts().await?)?;
    let rust_prompts = rust_prompts.as_array().unwrap();
    let python_prompts = values(&baseline, "prompts");
    assert_eq!(
        strings(rust_prompts, "name"),
        strings(python_prompts, "name")
    );
    for python_prompt in python_prompts {
        let name = python_prompt["name"].as_str().unwrap();
        let rust_prompt = named(rust_prompts, "name", name);
        assert_eq!(rust_prompt["description"], python_prompt["description"]);
        assert_eq!(
            prompt_arguments(rust_prompt),
            prompt_arguments(python_prompt),
            "{name} prompt arguments drifted"
        );
    }

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn approved_mcp_deltas_are_explicit() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT)?;
    let transport = TokioChildProcess::new(child_command())?;
    let client = ().serve(transport).await?;

    let python_instructions = baseline["initialize"]["instructions"].as_str().unwrap();
    assert!(python_instructions.contains("Provides real-time and historical"));
    assert_eq!(
        client.peer_info().unwrap().instructions.as_deref(),
        Some("Financial data server powered by Tiingo. Dates use YYYY-MM-DD.")
    );

    let rust_resources = client.list_all_resources().await?;
    assert!(
        rust_resources
            .iter()
            .all(|resource| resource.mime_type.as_deref() == Some("application/json"))
    );
    assert!(
        values(&baseline, "resources")
            .iter()
            .all(|resource| resource["mimeType"] == "text/plain")
    );

    let python_capabilities = baseline_resource_json(&baseline, "tiingo://capabilities");
    assert!(python_capabilities.get("rate_limits").is_some());
    assert!(python_capabilities.get("plan_restrictions").is_some());
    let rust_capabilities = serde_json::to_value(
        client
            .read_resource(ReadResourceRequestParams::new("tiingo://capabilities"))
            .await?,
    )?;
    let rust_capabilities: Value =
        serde_json::from_str(rust_capabilities["contents"][0]["text"].as_str().unwrap())?;
    assert_eq!(rust_capabilities["as_of"], "2026-08-24");
    assert_eq!(rust_capabilities["entitlements_change_over_time"], true);
    assert!(
        rust_capabilities["official_sources"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert!(rust_capabilities.get("rate_limits").is_none());
    assert!(rust_capabilities.get("plan_restrictions").is_none());

    for asset_class in [
        "corporate-actions",
        "crypto",
        "forex",
        "fundamentals",
        "news",
        "stocks",
    ] {
        let uri = format!("tiingo://guide/{asset_class}");
        let guide = serde_json::to_value(
            client
                .read_resource(ReadResourceRequestParams::new(&uri))
                .await?,
        )?;
        let guide: Value = serde_json::from_str(guide["contents"][0]["text"].as_str().unwrap())?;
        assert_eq!(guide["availability"]["as_of"], "2026-08-24");
        assert!(
            guide["availability"]["statement"]
                .as_str()
                .unwrap()
                .contains("current Tiingo account entitlements")
        );
        assert!(guide.get("plan_restrictions").is_none());
    }

    let python_stocks = baseline_resource_json(&baseline, "tiingo://guide/stocks");
    assert!(
        python_stocks["ticker_format"]
            .as_str()
            .unwrap()
            .contains("BRK.B")
    );
    let rust_stocks = serde_json::to_value(
        client
            .read_resource(ReadResourceRequestParams::new("tiingo://guide/stocks"))
            .await?,
    )?;
    let rust_stocks: Value =
        serde_json::from_str(rust_stocks["contents"][0]["text"].as_str().unwrap())?;
    assert!(
        rust_stocks["ticker_format"]
            .as_str()
            .unwrap()
            .contains("BRK-A")
    );
    assert!(
        !rust_stocks["ticker_format"]
            .as_str()
            .unwrap()
            .contains("BRK.B")
    );

    let python_analyze = result_text(&baseline["prompt_results"]["analyze-stock"]);
    assert!(!python_analyze.contains("get_company_meta"));
    let analyze = serde_json::to_value(
        client
            .get_prompt(
                GetPromptRequestParams::new("analyze-stock")
                    .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"}))),
            )
            .await?,
    )?;
    let analyze = result_text(&analyze);
    assert!(
        analyze.find("get_company_meta").unwrap() < analyze.find("sector and industry").unwrap()
    );

    let python_earnings = result_text(&baseline["prompt_results"]["earnings-report-analysis"]);
    assert!(python_earnings.contains("Beat or Miss"));
    assert!(python_earnings.contains("based on trends"));
    let earnings = serde_json::to_value(
        client
            .get_prompt(
                GetPromptRequestParams::new("earnings-report-analysis").with_arguments(arguments(
                    serde_json::json!({
                        "ticker": "NVDA",
                        "earnings_date": "2024-02-21"
                    }),
                )),
            )
            .await?,
    )?;
    let earnings = result_text(&earnings).to_lowercase();
    assert!(earnings.contains("expectations data is not available"));
    assert!(earnings.contains("do not label the results a beat or miss"));
    assert!(!earnings.contains("based on trends"));

    let python_forex = result_text(&baseline["prompt_results"]["forex-pair-analysis"]);
    assert!(python_forex.contains("likely causes"));
    let forex = serde_json::to_value(
        client
            .get_prompt(
                GetPromptRequestParams::new("forex-pair-analysis")
                    .with_arguments(arguments(serde_json::json!({"pair": "eurusd"}))),
            )
            .await?,
    )?;
    let forex = result_text(&forex);
    assert!(forex.contains("price data alone cannot establish their cause"));
    assert!(!forex.contains("likely causes"));

    let missing_key = client
        .call_tool(
            CallToolRequestParams::new("get_stock_metadata")
                .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"}))),
        )
        .await?;
    assert_eq!(missing_key.is_error, Some(true));
    assert_eq!(
        missing_key.structured_content.as_ref().unwrap()["error"]["kind"],
        "configuration"
    );
    let text = serde_json::to_value(&missing_key.content[0])?;
    let payload: Value = serde_json::from_str(text["text"].as_str().unwrap())?;
    assert_eq!(payload["kind"], "configuration");
    assert!(!payload.to_string().contains("TIINGO_API_KEY="));

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn approved_crypto_route_and_bounded_retry_delta_are_explicit() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT)?;
    let python_crypto_quote = values(&baseline, "http_requests")
        .iter()
        .find(|request| request["path"] == "/tiingo/crypto/top")
        .expect("frozen Python crypto quote route");
    assert_eq!(python_crypto_quote["method"], "GET");

    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/crypto/prices"))
        .and(query_param("tickers", "btcusd"))
        .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
        .up_to_n_times(2)
        .mount(&upstream)
        .await;
    Mock::given(method("GET"))
        .and(path("/tiingo/crypto/prices"))
        .and(query_param("tickers", "btcusd"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&upstream)
        .await;

    let client = TiingoClient::new(Config {
        api_key: Some("contract-key".to_owned()),
        base_url: Url::parse(&upstream.uri())?,
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    })?;
    assert_eq!(
        client.get_crypto_quote(Some("btcusd")).await?,
        serde_json::json!({"ok": true})
    );

    Ok(())
}
