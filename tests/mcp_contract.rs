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
const PYTHON_INITIALIZE_INSTRUCTIONS: &str = "Financial data server powered by Tiingo. Provides real-time and historical stock prices, forex rates, crypto data, news, fundamentals, and corporate actions. All date parameters use YYYY-MM-DD format.";
const RUST_INITIALIZE_INSTRUCTIONS: &str =
    "Financial data server powered by Tiingo. Dates use YYYY-MM-DD.";

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

fn remove_exact(object: &mut Map<String, Value>, key: &str, expected: Value) {
    let actual = object
        .remove(key)
        .unwrap_or_else(|| panic!("frozen contract lost {key}"));
    assert_eq!(actual, expected, "frozen contract changed {key}");
}

fn replace_exact(object: &mut Map<String, Value>, key: &str, expected: Value, replacement: Value) {
    remove_exact(object, key, expected);
    assert!(
        object.insert(key.to_owned(), replacement).is_none(),
        "{key} unexpectedly remained after exact removal"
    );
}

fn insert_delta(object: &mut Map<String, Value>, key: &str, value: Value) {
    assert!(
        !object.contains_key(key),
        "frozen contract unexpectedly already contains additive delta {key}"
    );
    object.insert(key.to_owned(), value);
}

fn replace_exact_once(text: &str, old: &str, new: &str) -> String {
    assert_eq!(
        text.matches(old).count(),
        1,
        "frozen contract changed approved text delta {old:?}"
    );
    text.replacen(old, new, 1)
}

fn normalize_protocol_metadata(result: &mut Map<String, Value>) {
    if let Some(result_type) = result.remove("resultType") {
        assert_eq!(result_type, "complete", "unexpected resultType shape");
    }
    if let Some(ttl_ms) = result.remove("ttlMs") {
        assert!(ttl_ms.as_u64().is_some(), "unexpected ttlMs shape");
    }
    if let Some(cache_scope) = result.remove("cacheScope") {
        assert!(
            matches!(cache_scope.as_str(), Some("public" | "private")),
            "unexpected cacheScope shape"
        );
    }
}

fn canonical_initialize(mut result: Value, expected: bool) -> Value {
    let object = result
        .as_object_mut()
        .expect("initialize result is an object");
    let server_info = object["serverInfo"]
        .as_object_mut()
        .expect("serverInfo is an object");
    assert!(
        server_info.get("version").is_some_and(Value::is_string),
        "implementation version must be a string"
    );
    server_info.insert("version".to_owned(), Value::String("<ignored>".to_owned()));
    if expected {
        replace_exact(
            object,
            "instructions",
            Value::String(PYTHON_INITIALIZE_INSTRUCTIONS.to_owned()),
            Value::String(RUST_INITIALIZE_INSTRUCTIONS.to_owned()),
        );
    }
    result
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

fn python_output_schema() -> Value {
    serde_json::json!({
        "description": "Generic wrapper for non-object return types.",
        "properties": {"result": {"type": "string"}},
        "required": ["result"],
        "type": "object",
        "x-fastmcp-wrap-result": true
    })
}

fn canonical_tools(mut tools: Vec<Value>, expected: bool) -> Vec<Value> {
    for tool in &mut tools {
        if expected {
            if tool["name"] == "get_crypto_quote" {
                let old = tool["description"].as_str().unwrap();
                let description = replace_exact_once(
                    old,
                    "Get current top-of-book crypto prices.",
                    "Get current crypto prices.",
                );
                tool["description"] = Value::String(description);
            }
            replace_exact(
                tool.as_object_mut().unwrap(),
                "outputSchema",
                python_output_schema(),
                structured_output_schema(),
            );
        }
        tool["inputSchema"] = normalize_schema(tool["inputSchema"].take());
        tool["outputSchema"] = normalize_schema(tool["outputSchema"].take());
    }
    sort_by_string_field(&mut tools, "name");
    tools
}

fn canonical_list(mut items: Vec<Value>, field: &str) -> Vec<Value> {
    sort_by_string_field(&mut items, field);
    items
}

fn canonical_resources(mut resources: Vec<Value>, expected: bool) -> Vec<Value> {
    if expected {
        for resource in &mut resources {
            replace_exact(
                resource.as_object_mut().unwrap(),
                "mimeType",
                Value::String("text/plain".to_owned()),
                Value::String("application/json".to_owned()),
            );
            if resource["uri"] == "tiingo://capabilities" {
                replace_exact(
                    resource.as_object_mut().unwrap(),
                    "description",
                    Value::String(
                        "Server capabilities, supported asset classes, rate limits, and plan restrictions"
                            .to_owned(),
                    ),
                    Value::String(
                        "Server capabilities and source-dated entitlement guidance".to_owned(),
                    ),
                );
            }
        }
    }
    canonical_list(resources, "uri")
}

fn canonical_resource_templates(mut templates: Vec<Value>, expected: bool) -> Vec<Value> {
    if expected {
        for template in &mut templates {
            replace_exact(
                template.as_object_mut().unwrap(),
                "mimeType",
                Value::String("text/plain".to_owned()),
                Value::String("application/json".to_owned()),
            );
        }
    }
    canonical_list(templates, "uriTemplate")
}

fn expected_resource_body(uri: &str, mut body: Value) -> Value {
    let object = body.as_object_mut().unwrap();
    match uri {
        "tiingo://capabilities" => {
            remove_exact(object, "server_version", serde_json::json!("1.1.0"));
            remove_exact(
                object,
                "rate_limits",
                serde_json::json!({"free": "50 req/hr", "power": "5000 req/hr"}),
            );
            remove_exact(
                object,
                "plan_restrictions",
                serde_json::json!({
                    "free_tier": [
                        "get_stock_metadata",
                        "get_stock_prices",
                        "get_realtime_price",
                        "get_intraday_prices",
                        "get_forex_quote",
                        "get_forex_prices",
                        "get_crypto_quote",
                        "get_crypto_prices",
                        "get_crypto_metadata",
                        "get_news",
                        "get_fundamentals_definitions",
                        "get_financial_statements",
                        "get_daily_fundamentals",
                        "get_company_meta",
                        "get_dividend_yield"
                    ],
                    "paid_tier_required": ["get_dividends", "get_splits"]
                }),
            );
            insert_delta(object, "as_of", Value::String(SOURCE_DATE.to_owned()));
            insert_delta(object, "entitlements_change_over_time", Value::Bool(true));
            insert_delta(
                object,
                "official_sources",
                serde_json::json!(OFFICIAL_SOURCES),
            );
        }
        "tiingo://guide/corporate-actions"
        | "tiingo://guide/crypto"
        | "tiingo://guide/forex"
        | "tiingo://guide/fundamentals"
        | "tiingo://guide/news"
        | "tiingo://guide/stocks" => {
            let old_restriction = match uri {
                "tiingo://guide/corporate-actions" => {
                    "get_dividends and get_splits return 403 on free tier; get_dividend_yield is available on free tier."
                }
                "tiingo://guide/crypto" | "tiingo://guide/forex" | "tiingo://guide/news" => {
                    "Available on free tier."
                }
                "tiingo://guide/fundamentals" => {
                    "All fundamentals endpoints available on free tier."
                }
                "tiingo://guide/stocks" => "All stock endpoints available on free tier.",
                _ => unreachable!(),
            };
            remove_exact(
                object,
                "plan_restrictions",
                Value::String(old_restriction.to_owned()),
            );
            insert_delta(
                object,
                "availability",
                serde_json::json!({
                    "as_of": SOURCE_DATE,
                    "official_sources": OFFICIAL_SOURCES,
                    "statement": "Access depends on current Tiingo account entitlements; a 403 means this credential is not entitled to the requested capability."
                }),
            );
            if uri == "tiingo://guide/corporate-actions" {
                let pitfalls = object["common_pitfalls"].as_array_mut().unwrap();
                assert_eq!(
                    pitfalls.first(),
                    Some(&serde_json::json!(
                        "get_dividends and get_splits require a paid plan -- free tier returns 403."
                    )),
                    "frozen corporate-actions entitlement pitfall changed"
                );
                pitfalls.remove(0);
            } else if uri == "tiingo://guide/crypto" {
                insert_delta(
                    object,
                    "current_price_route",
                    Value::String("/tiingo/crypto/prices".to_owned()),
                );
            } else if uri == "tiingo://guide/stocks" {
                replace_exact(
                    object,
                    "ticker_format",
                    Value::String("Uppercase symbols, e.g. AAPL, MSFT, GOOGL, BRK.B".to_owned()),
                    Value::String("Uppercase symbols, e.g. AAPL, MSFT, GOOGL, BRK-A".to_owned()),
                );
            }
        }
        _ => {}
    }
    body
}

fn canonical_resource_result(uri: &str, mut result: Value, expected: bool) -> Value {
    normalize_protocol_metadata(result.as_object_mut().unwrap());
    for content in result["contents"].as_array_mut().unwrap() {
        if expected {
            replace_exact(
                content.as_object_mut().unwrap(),
                "mimeType",
                Value::String("text/plain".to_owned()),
                Value::String("application/json".to_owned()),
            );
        }
        let mut body: Value = serde_json::from_str(content["text"].as_str().unwrap()).unwrap();
        if expected {
            body = expected_resource_body(uri, body);
        } else if uri == "tiingo://capabilities" {
            let version = body
                .as_object_mut()
                .unwrap()
                .remove("server_version")
                .expect("capabilities server_version is present");
            assert!(
                version.is_string(),
                "capabilities server_version is a string"
            );
        }
        content["text"] = body;
    }
    result
}

fn corrected_prompt_text(name: &str, text: &str) -> String {
    match name {
        "analyze-stock" => replace_exact_once(
            &replace_exact_once(
                &replace_exact_once(
                    &replace_exact_once(
                        &replace_exact_once(
                            text,
                            "2. Call get_stock_prices",
                            "2. Call get_company_meta with tickers=AAPL to retrieve sector and industry.\n3. Call get_stock_prices",
                        ),
                        "identify key catalysts, analyst commentary, and market-moving events",
                        "identify reported catalysts and market-moving events",
                    ),
                    "- **Recent Catalysts**: Key news stories or events driving price movement (if news was fetched).",
                    "- **Recent Catalysts**: Reported news or events if news was fetched; do not infer causes absent evidence.",
                ),
                "\n3. Call get_daily_fundamentals",
                "\n4. Call get_daily_fundamentals",
            ),
            "\n4. Call get_news",
            "\n5. Call get_news",
        ),
        "earnings-report-analysis" => replace_exact_once(
            text,
            "- **Beat or Miss**: Did the company beat or miss expectations based on trends?",
            "- **Expectations Context**: Do not label the result a beat or miss unless an article supplies an explicit consensus comparison.",
        ),
        "forex-pair-analysis" => replace_exact_once(
            text,
            "- **Notable Moves**: Any significant spikes or drops and their likely causes.",
            "- **Notable Moves**: Identify significant spikes or drops, but state that price history alone cannot establish their cause.",
        ),
        _ => text.to_owned(),
    }
}

fn canonical_prompt_result(name: &str, mut result: Value, expected: bool) -> Value {
    normalize_protocol_metadata(result.as_object_mut().unwrap());
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

    let expected_initialize = canonical_initialize(baseline["initialize"].clone(), true);
    let actual_initialize =
        canonical_initialize(serde_json::to_value(client.peer_info().unwrap())?, false);
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
        canonical_resources(actual_resources, false),
        canonical_resources(baseline["resources"].as_array().unwrap().clone(), true),
        "resource discovery drifted"
    );
    let actual_templates = serde_json::to_value(client.list_all_resource_templates().await?)?
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        canonical_resource_templates(actual_templates, false),
        canonical_resource_templates(
            baseline["resource_templates"].as_array().unwrap().clone(),
            true
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

#[test]
fn canonicalization_preserves_unapproved_metadata() {
    let items = vec![serde_json::json!({
        "name": "example",
        "_meta": {"future-extension": {"enabled": true}}
    })];

    assert_eq!(canonical_list(items.clone(), "name"), items);
}

#[test]
fn approved_resource_discovery_delta_rejects_mutated_frozen_source() {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT).unwrap();
    let mut resources = baseline["resources"].as_array().unwrap().clone();
    resources[0]["mimeType"] = Value::String("application/json".to_owned());
    assert_transform_rejects("resource MIME type", move || {
        canonical_resources(resources, true);
    });

    let mut resources = baseline["resources"].as_array().unwrap().clone();
    resources[0]["description"] =
        Value::String("Server capabilities and source-dated entitlement guidance".to_owned());
    assert_transform_rejects("capabilities description", move || {
        canonical_resources(resources, true);
    });

    let mut templates = baseline["resource_templates"].as_array().unwrap().clone();
    templates[0]["mimeType"] = Value::String("application/json".to_owned());
    assert_transform_rejects("resource template MIME type", move || {
        canonical_resource_templates(templates, true);
    });

    let mut contents = serde_json::json!({
        "contents": baseline["resource_contents"]["tiingo://capabilities"].clone()
    });
    contents["contents"][0]["mimeType"] = Value::String("application/json".to_owned());
    assert_transform_rejects("resource content MIME type", move || {
        canonical_resource_result("tiingo://capabilities", contents, true);
    });
}

#[test]
fn approved_output_schema_delta_rejects_a_mutated_frozen_source() {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT).unwrap();
    let mut tools = baseline["tools"].as_array().unwrap().clone();
    tools[0]["outputSchema"]["properties"]["result"]["type"] = Value::String("number".into());

    assert!(
        std::panic::catch_unwind(|| canonical_tools(tools, true)).is_err(),
        "canonicalization overwrote a mutated frozen output schema"
    );
}

fn frozen_resource_body(baseline: &Value, uri: &str) -> Value {
    serde_json::from_str(
        baseline["resource_contents"][uri][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

fn assert_transform_rejects(label: &str, transform: impl FnOnce() + std::panic::UnwindSafe) {
    assert!(
        std::panic::catch_unwind(transform).is_err(),
        "canonicalization accepted mutated frozen {label}"
    );
}

#[test]
fn every_approved_delta_rejects_mutated_frozen_values() {
    let baseline: Value = serde_json::from_str(PYTHON_CONTRACT).unwrap();

    let mut initialize = baseline["initialize"].clone();
    initialize["instructions"] = Value::String(RUST_INITIALIZE_INSTRUCTIONS.to_owned());
    assert_transform_rejects("initialize instructions", move || {
        canonical_initialize(initialize, true);
    });

    let mut tools = baseline["tools"].as_array().unwrap().clone();
    let crypto = tools
        .iter_mut()
        .find(|tool| tool["name"] == "get_crypto_quote")
        .unwrap();
    crypto["description"] = Value::String(
        crypto["description"]
            .as_str()
            .unwrap()
            .replace("top-of-book crypto", "crypto"),
    );
    assert_transform_rejects("crypto description", move || {
        canonical_tools(tools, true);
    });

    for key in [
        "server_version",
        "rate_limits",
        "plan_restrictions",
        "as_of",
        "entitlements_change_over_time",
        "official_sources",
    ] {
        let mut body = frozen_resource_body(&baseline, "tiingo://capabilities");
        body[key] = Value::String("mutated".to_owned());
        assert_transform_rejects(key, move || {
            expected_resource_body("tiingo://capabilities", body);
        });
    }

    for uri in [
        "tiingo://guide/corporate-actions",
        "tiingo://guide/crypto",
        "tiingo://guide/forex",
        "tiingo://guide/fundamentals",
        "tiingo://guide/news",
        "tiingo://guide/stocks",
    ] {
        let mut body = frozen_resource_body(&baseline, uri);
        body["plan_restrictions"] = Value::String("mutated".to_owned());
        assert_transform_rejects(uri, move || {
            expected_resource_body(uri, body);
        });
    }

    let mut corporate = frozen_resource_body(&baseline, "tiingo://guide/corporate-actions");
    corporate["common_pitfalls"][0] = Value::String("mutated".to_owned());
    assert_transform_rejects("corporate-actions pitfall", move || {
        expected_resource_body("tiingo://guide/corporate-actions", corporate);
    });

    let mut crypto = frozen_resource_body(&baseline, "tiingo://guide/crypto");
    crypto["current_price_route"] = Value::String("/mutated".to_owned());
    assert_transform_rejects("crypto current route", move || {
        expected_resource_body("tiingo://guide/crypto", crypto);
    });

    let mut stocks = frozen_resource_body(&baseline, "tiingo://guide/stocks");
    stocks["ticker_format"] = Value::String("mutated".to_owned());
    assert_transform_rejects("stock symbology", move || {
        expected_resource_body("tiingo://guide/stocks", stocks);
    });

    let mut guide = frozen_resource_body(&baseline, "tiingo://guide/news");
    guide["availability"] = serde_json::json!({"mutated": true});
    assert_transform_rejects("availability source metadata", move || {
        expected_resource_body("tiingo://guide/news", guide);
    });

    for (name, old, replacement) in [
        (
            "analyze-stock",
            "identify key catalysts, analyst commentary, and market-moving events",
            "identify mutated catalysts",
        ),
        (
            "earnings-report-analysis",
            "- **Beat or Miss**: Did the company beat or miss expectations based on trends?",
            "- **Mutated**",
        ),
        (
            "forex-pair-analysis",
            "- **Notable Moves**: Any significant spikes or drops and their likely causes.",
            "- **Mutated**",
        ),
    ] {
        let text = baseline["prompt_results"][name]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .replace(old, replacement);
        assert_transform_rejects(name, move || {
            corrected_prompt_text(name, &text);
        });
    }

    let text = baseline["prompt_results"]["analyze-stock"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap()
        .replace(
            "- **Recent Catalysts**: Key news stories or events driving price movement (if news was fetched).",
            "- **Recent Catalysts**: Mutated.",
        );
    assert_transform_rejects("analyze-stock evidence qualification", move || {
        corrected_prompt_text("analyze-stock", &text);
    });
}
