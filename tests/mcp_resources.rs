use std::time::Duration;

use rmcp::{
    RoleClient, ServiceExt,
    model::{ReadResourceRequestParams, ResourceContents},
    service::{RunningService, ServiceError},
};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    mcp::TiingoServer,
};
use url::Url;

const FIXED_URIS: [&str; 3] = [
    "tiingo://capabilities",
    "tiingo://fundamentals/definitions",
    "tiingo://guide/date-formats",
];
const GUIDES: [&str; 10] = [
    "corporate-actions",
    "crypto",
    "crypto-yield",
    "forex",
    "funds",
    "fundamentals",
    "market-data",
    "news",
    "search",
    "stocks",
];

struct Connection {
    client: RunningService<RoleClient, ()>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Connection {
    async fn new() -> Self {
        let client = TiingoClient::new(Config {
            api_key: None,
            base_url: Url::parse("http://127.0.0.1:1").unwrap(),
            request_timeout: Duration::from_secs(1),
            retry: RetryPolicy::test(),
            max_response_bytes: 8 * 1024 * 1024,
        })
        .unwrap();
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let server = tokio::spawn(async move {
            TiingoServer::with_client(client)
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });
        Self {
            client: ().serve(client_transport).await.unwrap(),
            server,
        }
    }

    async fn close(mut self) {
        self.client.close().await.unwrap();
        self.server.await.unwrap().unwrap();
    }
}

fn json_text(result: &rmcp::model::ReadResourceResult) -> serde_json::Value {
    let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] else {
        panic!("resource must return text content");
    };
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn advertises_and_reads_corrected_legacy_resources() {
    let connection = Connection::new().await;

    let resources = connection
        .client
        .list_resources(None)
        .await
        .unwrap()
        .resources;
    assert_eq!(resources.len(), FIXED_URIS.len());
    assert_eq!(
        resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>(),
        FIXED_URIS,
    );
    assert!(
        resources
            .iter()
            .all(|resource| resource.mime_type.as_deref() == Some("application/json"))
    );
    assert_eq!(
        resources[0].description.as_deref(),
        Some("Server capabilities and source-dated entitlement guidance")
    );

    let templates = connection
        .client
        .list_resource_templates(None)
        .await
        .unwrap()
        .resource_templates;
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].uri_template, "tiingo://guide/{asset_class}");
    assert_eq!(templates[0].mime_type.as_deref(), Some("application/json"));

    let capabilities = connection
        .client
        .read_resource(ReadResourceRequestParams::new("tiingo://capabilities"))
        .await
        .unwrap();
    let ResourceContents::TextResourceContents { mime_type, .. } = &capabilities.contents[0] else {
        panic!("capabilities must return text content");
    };
    assert_eq!(mime_type.as_deref(), Some("application/json"));
    let capabilities = json_text(&capabilities);
    let tools = connection.client.list_tools(None).await.unwrap().tools;
    assert_eq!(capabilities["server_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        capabilities["tool_count"].as_u64(),
        Some(tools.len() as u64)
    );
    assert_eq!(capabilities["as_of"], "2026-08-25");
    let capability_sources = capabilities["official_sources"].as_array().unwrap();
    for source in [
        "https://www.tiingo.com/documentation/general/overview",
        "https://www.tiingo.com/documentation/end-of-day",
        "https://www.tiingo.com/documentation/websockets/iex",
        "https://www.tiingo.com/documentation/websockets/equity-realtime-stock-data",
    ] {
        assert!(
            capability_sources.iter().any(|value| value == source),
            "capabilities must cite {source}"
        );
    }

    for asset_class in GUIDES {
        let result = connection
            .client
            .read_resource(ReadResourceRequestParams::new(format!(
                "tiingo://guide/{asset_class}"
            )))
            .await
            .unwrap();
        let ResourceContents::TextResourceContents { mime_type, .. } = &result.contents[0] else {
            panic!("guide must return text content");
        };
        assert_eq!(mime_type.as_deref(), Some("application/json"));
        let guide = json_text(&result);
        assert!(guide.is_object(), "{asset_class} must return JSON");
        assert_eq!(guide["availability"]["as_of"], "2026-08-25");
        assert!(
            guide["availability"]["official_sources"]
                .as_array()
                .is_some_and(|sources| !sources.is_empty())
        );
        let endpoint_documentation = match asset_class {
            "crypto" => Some("https://www.tiingo.com/documentation/crypto"),
            "crypto-yield" => Some("https://www.tiingo.com/documentation/general/overview"),
            "forex" => Some("https://www.tiingo.com/documentation/forex"),
            "funds" => Some("https://www.tiingo.com/documentation/mutual-fund-and-etf-fees"),
            "fundamentals" => Some("https://www.tiingo.com/documentation/fundamentals"),
            "market-data" => Some("https://www.tiingo.com/documentation/websockets/iex"),
            "news" => Some("https://www.tiingo.com/documentation/news"),
            "search" => Some("https://www.tiingo.com/documentation/utilities/search"),
            _ => None,
        };
        if let Some(endpoint_documentation) = endpoint_documentation {
            assert!(
                guide["availability"]["official_sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|source| source == endpoint_documentation),
                "{asset_class} must link to its matching Tiingo documentation"
            );
        }
        if asset_class == "fundamentals" {
            assert_eq!(
                guide["common_pitfalls"][0],
                "Financial statements are reported quarterly and annually; don't expect daily granularity."
            );
        }
    }

    let invalid = connection
        .client
        .read_resource(ReadResourceRequestParams::new("tiingo://guide/invalid"))
        .await
        .unwrap();
    let ResourceContents::TextResourceContents { mime_type, .. } = &invalid.contents[0] else {
        panic!("invalid guide response must return text content");
    };
    assert_eq!(mime_type.as_deref(), Some("application/json"));
    assert_eq!(
        json_text(&invalid),
        serde_json::json!({
            "error": "Invalid asset class 'invalid'. Valid values: corporate-actions, crypto, crypto-yield, forex, funds, fundamentals, market-data, news, search, stocks"
        })
    );

    let missing = connection
        .client
        .read_resource(ReadResourceRequestParams::new("tiingo://unknown"))
        .await
        .unwrap_err();
    let ServiceError::McpError(missing) = missing else {
        panic!("unknown resource must return an MCP error");
    };
    assert_eq!(missing.code, rmcp::model::ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(missing.message, "resource not found");
    assert_eq!(
        missing.data,
        Some(serde_json::json!({"uri": "tiingo://unknown"}))
    );

    let mut contents = vec![capabilities.to_string()];
    for uri in FIXED_URIS
        .into_iter()
        .filter(|uri| *uri != "tiingo://capabilities")
    {
        let result = connection
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .unwrap();
        let ResourceContents::TextResourceContents { mime_type, .. } = &result.contents[0] else {
            panic!("fixed resource must return text content");
        };
        assert_eq!(mime_type.as_deref(), Some("application/json"));
        contents.push(json_text(&result).to_string());
    }
    for asset_class in GUIDES {
        let result = connection
            .client
            .read_resource(ReadResourceRequestParams::new(format!(
                "tiingo://guide/{asset_class}"
            )))
            .await
            .unwrap();
        contents.push(json_text(&result).to_string());
    }
    let contents = contents.join("\n");
    for stale_claim in [
        "5000 req/hr",
        "50,000",
        "All fundamentals endpoints available on free tier",
        "BRK.B",
    ] {
        assert!(
            !contents.contains(stale_claim),
            "found stale claim: {stale_claim}"
        );
    }
    assert!(contents.contains("BRK-A"));

    connection.close().await;
}
