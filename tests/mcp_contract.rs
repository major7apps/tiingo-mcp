use std::{cmp::Ordering, time::Duration};

use anyhow::Context;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, GetPromptRequestParams, JsonObject, ReadResourceRequestParams},
    transport::TokioChildProcess,
};
use serde_json::{Map, Value};
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
const CHILD_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_DATE: &str = "2026-08-24";
const OFFICIAL_SOURCES: [&str; 2] = [
    "https://www.tiingo.com/documentation/general/overview",
    "https://api.tiingo.com/documentation/end-of-day",
];

fn child_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tiingo-mcp"));
    command.env_remove("TIINGO_API_KEY");
    command
}

fn arguments(value: Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

fn sort_by_string_field(items: &mut [Value], field: &str) {
    items.sort_by(|left, right| {
        left[field]
            .as_str()
            .partial_cmp(&right[field].as_str())
            .unwrap_or(Ordering::Equal)
    });
}

fn normalize_schema(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(normalize_schema).collect()),
        Value::Object(mut object) => {
            object.remove("$schema");
            for value in object.values_mut() {
                *value = normalize_schema(value.take());
            }
            if let Some(branches) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
                branches.sort_by_key(Value::to_string);
            }
            if let Some(branches) = object.get("anyOf").and_then(Value::as_array)
                && branches.iter().all(|branch| {
                    branch
                        .as_object()
                        .is_some_and(|branch| branch.len() == 1 && branch["type"].is_string())
                })
            {
                let mut types = branches
                    .iter()
                    .map(|branch| branch["type"].clone())
                    .collect::<Vec<_>>();
                types.sort_by_key(Value::to_string);
                object.remove("anyOf");
                object.insert("type".to_owned(), Value::Array(types));
            }
            for key in ["enum", "required", "type"] {
                if let Some(values) = object.get_mut(key).and_then(Value::as_array_mut) {
                    values.sort_by_key(Value::to_string);
                }
            }
            Value::Object(object)
        }
        value => value,
    }
}

fn structured_output_schema() -> Value {
    serde_json::json!({
        "additionalProperties": false,
        "properties": {
            "data": {},
            "meta": {
                "additionalProperties": false,
                "properties": {"source": {"type": "string"}},
                "required": ["source"],
                "type": "object"
            }
        },
        "required": ["data", "meta"],
        "type": "object"
    })
}

fn canonical_tools(mut tools: Vec<Value>, expected: bool) -> Vec<Value> {
    for tool in &mut tools {
        tool.as_object_mut().unwrap().remove("_meta");
        if expected {
            if tool["name"] == "get_crypto_quote" {
                let description = tool["description"].as_str().unwrap().replacen(
                    "Get current top-of-book crypto prices.",
                    "Get current crypto prices.",
                    1,
                );
                tool["description"] = Value::String(description);
            }
            tool["outputSchema"] = structured_output_schema();
        }
        tool["inputSchema"] = normalize_schema(tool["inputSchema"].take());
        tool["outputSchema"] = normalize_schema(tool["outputSchema"].take());
    }
    sort_by_string_field(&mut tools, "name");
    tools
}

fn canonical_list(mut items: Vec<Value>, field: &str) -> Vec<Value> {
    for item in &mut items {
        item.as_object_mut().unwrap().remove("_meta");
    }
    sort_by_string_field(&mut items, field);
    items
}

fn expected_resource_body(uri: &str, mut body: Value) -> Value {
    let object = body.as_object_mut().unwrap();
    match uri {
        "tiingo://capabilities" => {
            object.remove("server_version");
            object.remove("rate_limits");
            object.remove("plan_restrictions");
            object.insert("as_of".to_owned(), Value::String(SOURCE_DATE.to_owned()));
            object.insert(
                "entitlements_change_over_time".to_owned(),
                Value::Bool(true),
            );
            object.insert(
                "official_sources".to_owned(),
                serde_json::json!(OFFICIAL_SOURCES),
            );
        }
        "tiingo://guide/corporate-actions"
        | "tiingo://guide/crypto"
        | "tiingo://guide/forex"
        | "tiingo://guide/fundamentals"
        | "tiingo://guide/news"
        | "tiingo://guide/stocks" => {
            object.remove("plan_restrictions");
            object.insert(
                "availability".to_owned(),
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": OFFICIAL_SOURCES,
                    "statement": "Access depends on current Tiingo account entitlements; a 403 means this credential is not entitled to the requested capability."
                }),
            );
            if uri == "tiingo://guide/corporate-actions" {
                object["common_pitfalls"].as_array_mut().unwrap().remove(0);
            } else if uri == "tiingo://guide/crypto" {
                object.insert(
                    "current_price_route".to_owned(),
                    Value::String("/tiingo/crypto/prices".to_owned()),
                );
            } else if uri == "tiingo://guide/stocks" {
                let ticker_format = object["ticker_format"]
                    .as_str()
                    .unwrap()
                    .replace("BRK.B", "BRK-A");
                object.insert("ticker_format".to_owned(), Value::String(ticker_format));
            }
        }
        _ => {}
    }
    body
}

fn canonical_resource_result(uri: &str, mut result: Value, expected: bool) -> Value {
    result.as_object_mut().unwrap().remove("resultType");
    result.as_object_mut().unwrap().remove("_meta");
    for content in result["contents"].as_array_mut().unwrap() {
        content.as_object_mut().unwrap().remove("_meta");
        let mut body: Value = serde_json::from_str(content["text"].as_str().unwrap()).unwrap();
        if expected {
            body = expected_resource_body(uri, body);
        } else if uri == "tiingo://capabilities" {
            body.as_object_mut().unwrap().remove("server_version");
        }
        content["text"] = body;
    }
    result
}

fn corrected_prompt_text(name: &str, text: &str) -> String {
    match name {
        "analyze-stock" => text
            .replace(
                "2. Call get_stock_prices",
                "2. Call get_company_meta with tickers=AAPL to retrieve sector and industry.\n3. Call get_stock_prices",
            )
            .replace("\n3. Call get_daily_fundamentals", "\n4. Call get_daily_fundamentals")
            .replace("\n4. Call get_news", "\n5. Call get_news"),
        "earnings-report-analysis" => text.replace(
            "- **Beat or Miss**: Did the company beat or miss expectations based on trends?",
            "- **Expectations Context**: Do not label the result a beat or miss unless an article supplies an explicit consensus comparison.",
        ),
        "forex-pair-analysis" => text.replace(
            "- **Notable Moves**: Any significant spikes or drops and their likely causes.",
            "- **Notable Moves**: Identify significant spikes or drops, but state that price history alone cannot establish their cause.",
        ),
        _ => text.to_owned(),
    }
}

fn canonical_prompt_result(name: &str, mut result: Value, expected: bool) -> Value {
    result.as_object_mut().unwrap().remove("resultType");
    result.as_object_mut().unwrap().remove("_meta");
    if expected {
        let text = result["messages"][0]["content"]["text"].as_str().unwrap();
        result["messages"][0]["content"]["text"] = Value::String(corrected_prompt_text(name, text));
    }
    result
}

fn prompt_requests() -> [(String, Map<String, Value>); 5] {
    [
        (
            "analyze-stock".to_owned(),
            arguments(serde_json::json!({"ticker": "AAPL"})),
        ),
        (
            "compare-stocks".to_owned(),
            arguments(serde_json::json!({"ticker1": "AAPL", "ticker2": "MSFT"})),
        ),
        ("crypto-market-overview".to_owned(), JsonObject::new()),
        (
            "earnings-report-analysis".to_owned(),
            arguments(serde_json::json!({
                "ticker": "NVDA",
                "earnings_date": "2024-02-21"
            })),
        ),
        (
            "forex-pair-analysis".to_owned(),
            arguments(serde_json::json!({"pair": "eurusd"})),
        ),
    ]
}

async fn child_process_contract() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT)?;
    let client = ().serve(TokioChildProcess::new(child_command())?).await?;

    assert_eq!(client.list_all_tools().await?.len(), 17);
    assert_eq!(client.list_all_resources().await?.len(), 3);
    assert_eq!(client.list_all_resource_templates().await?.len(), 1);
    assert_eq!(client.list_all_prompts().await?.len(), 5);

    let mut expected_initialize = baseline["initialize"].clone();
    expected_initialize["serverInfo"]["version"] = Value::String("<ignored>".to_owned());
    expected_initialize["instructions"] =
        Value::String("Financial data server powered by Tiingo. Dates use YYYY-MM-DD.".to_owned());
    let mut actual_initialize = serde_json::to_value(client.peer_info().unwrap())?;
    actual_initialize["serverInfo"]["version"] = Value::String("<ignored>".to_owned());
    assert_eq!(actual_initialize, expected_initialize, "initialize drifted");

    let actual_tools = serde_json::to_value(client.list_all_tools().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_tools(actual_tools, false),
        canonical_tools(baseline["tools"].as_array().unwrap().clone(), true),
        "tool discovery drifted"
    );

    let actual_resources = serde_json::to_value(client.list_all_resources().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_list(actual_resources, "uri"),
        canonical_list(baseline["resources"].as_array().unwrap().clone(), "uri"),
        "resource discovery drifted"
    );
    let actual_templates = serde_json::to_value(client.list_all_resource_templates().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_list(actual_templates, "uriTemplate"),
        canonical_list(
            baseline["resource_templates"].as_array().unwrap().clone(),
            "uriTemplate"
        ),
        "resource-template discovery drifted"
    );
    let actual_prompts = serde_json::to_value(client.list_all_prompts().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_list(actual_prompts, "name"),
        canonical_list(baseline["prompts"].as_array().unwrap().clone(), "name"),
        "prompt discovery drifted"
    );

    for (uri, expected) in baseline["resource_contents"].as_object().unwrap() {
        let actual = serde_json::to_value(
            client
                .read_resource(ReadResourceRequestParams::new(uri))
                .await?,
        )?;
        assert_eq!(
            canonical_resource_result(uri, actual, false),
            canonical_resource_result(uri, serde_json::json!({"contents": expected.clone()}), true),
            "{uri} content drifted"
        );
    }

    for (name, request_arguments) in prompt_requests() {
        let actual = serde_json::to_value(
            client
                .get_prompt(GetPromptRequestParams::new(&name).with_arguments(request_arguments))
                .await?,
        )?;
        assert_eq!(
            canonical_prompt_result(&name, actual, false),
            canonical_prompt_result(&name, baseline["prompt_results"][&name].clone(), true),
            "{name} result drifted"
        );
    }

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
async fn child_process_matches_the_frozen_contract_plus_exact_approved_deltas() -> anyhow::Result<()>
{
    tokio::time::timeout(CHILD_TIMEOUT, child_process_contract())
        .await
        .context("compiled child contract timed out")??;
    Ok(())
}

#[test]
fn frozen_python_rejected_current_discover_with_the_exact_legacy_error() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT)?;
    assert_eq!(
        baseline["server_discover"],
        serde_json::json!({
            "error": {
                "code": -32602,
                "data": "",
                "message": "Invalid request parameters"
            }
        })
    );
    Ok(())
}

#[tokio::test]
async fn approved_crypto_route_and_bounded_retry_delta_are_explicit() -> anyhow::Result<()> {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT)?;
    assert!(
        baseline["http_requests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|request| request["path"] == "/tiingo/crypto/top")
    );

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
