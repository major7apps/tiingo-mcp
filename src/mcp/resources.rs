use rmcp::{
    ErrorData,
    model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate},
};

use super::compatibility_descriptor_meta;

const CAPABILITIES: &str = include_str!("data/capabilities.json");
const DEFINITIONS: &str = include_str!("data/fundamentals-definitions.json");
const DATE_FORMATS: &str = include_str!("data/date-formats.json");
const GUIDE_CORPORATE_ACTIONS: &str = include_str!("data/guides/corporate-actions.json");
const GUIDE_CRYPTO: &str = include_str!("data/guides/crypto.json");
const GUIDE_FOREX: &str = include_str!("data/guides/forex.json");
const GUIDE_FUNDAMENTALS: &str = include_str!("data/guides/fundamentals.json");
const GUIDE_NEWS: &str = include_str!("data/guides/news.json");
const GUIDE_STOCKS: &str = include_str!("data/guides/stocks.json");

pub fn list() -> Vec<Resource> {
    vec![
        Resource::new("tiingo://capabilities", "capabilities_resource")
            .with_description("Server capabilities and source-dated entitlement guidance")
            .with_mime_type("application/json")
            .with_meta(compatibility_descriptor_meta()),
        Resource::new(
            "tiingo://fundamentals/definitions",
            "fundamentals_definitions_resource",
        )
        .with_description(
            "Curated reference of common fundamental metrics with descriptions and statement types",
        )
        .with_mime_type("application/json")
        .with_meta(compatibility_descriptor_meta()),
        Resource::new("tiingo://guide/date-formats", "date_formats_resource")
            .with_description(
                "Date format, resample frequencies, sort options, and parameter reference for all endpoints",
            )
            .with_mime_type("application/json")
            .with_meta(compatibility_descriptor_meta()),
    ]
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new(
            "tiingo://guide/{asset_class}",
            "asset_class_guide_resource",
        )
        .with_description(
            "Usage guide for a specific asset class: tools, workflows, ticker formats, and pitfalls",
        )
        .with_mime_type("application/json")
        .with_meta(compatibility_descriptor_meta()),
    ]
}

pub fn read(uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let json = match uri {
        "tiingo://capabilities" => {
            let mut value: serde_json::Value = serde_json::from_str(CAPABILITIES)
                .expect("embedded capabilities JSON must be valid");
            value["server_version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
            serde_json::to_string(&value).expect("capabilities JSON must serialize")
        }
        "tiingo://fundamentals/definitions" => DEFINITIONS.to_owned(),
        "tiingo://guide/date-formats" => DATE_FORMATS.to_owned(),
        "tiingo://guide/corporate-actions" => GUIDE_CORPORATE_ACTIONS.to_owned(),
        "tiingo://guide/crypto" => GUIDE_CRYPTO.to_owned(),
        "tiingo://guide/forex" => GUIDE_FOREX.to_owned(),
        "tiingo://guide/fundamentals" => GUIDE_FUNDAMENTALS.to_owned(),
        "tiingo://guide/news" => GUIDE_NEWS.to_owned(),
        "tiingo://guide/stocks" => GUIDE_STOCKS.to_owned(),
        value if value.starts_with("tiingo://guide/") => {
            let name = &value["tiingo://guide/".len()..];
            let error = serde_json::json!({
                "error": format!(
                    "Invalid asset class '{name}'. Valid values: corporate-actions, crypto, forex, fundamentals, news, stocks"
                )
            });
            return Ok(ReadResourceResult::new(vec![
                ResourceContents::text(error.to_string(), uri).with_mime_type("application/json"),
            ]));
        }
        _ => {
            return Err(ErrorData::resource_not_found(
                "resource not found",
                Some(serde_json::json!({ "uri": uri })),
            ));
        }
    };
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(json, uri).with_mime_type("application/json"),
    ]))
}
