use rmcp::{
    ErrorData,
    model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate},
};

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
        Resource::new("tiingo://capabilities", "capabilities")
            .with_description("Server capabilities and source-dated entitlement guidance")
            .with_mime_type("application/json"),
        Resource::new(
            "tiingo://fundamentals/definitions",
            "fundamentals-definitions",
        )
        .with_description("Curated common fundamental metric definitions")
        .with_mime_type("application/json"),
        Resource::new("tiingo://guide/date-formats", "date-formats")
            .with_description("Date, resampling, sorting, and corporate-action parameter reference")
            .with_mime_type("application/json"),
    ]
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("tiingo://guide/{asset_class}", "asset-class-guide")
            .with_description("Tools, workflows, symbology, and pitfalls for one asset class")
            .with_mime_type("application/json"),
    ]
}

pub fn read(uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let json = match uri {
        "tiingo://capabilities" => CAPABILITIES,
        "tiingo://fundamentals/definitions" => DEFINITIONS,
        "tiingo://guide/date-formats" => DATE_FORMATS,
        "tiingo://guide/corporate-actions" => GUIDE_CORPORATE_ACTIONS,
        "tiingo://guide/crypto" => GUIDE_CRYPTO,
        "tiingo://guide/forex" => GUIDE_FOREX,
        "tiingo://guide/fundamentals" => GUIDE_FUNDAMENTALS,
        "tiingo://guide/news" => GUIDE_NEWS,
        "tiingo://guide/stocks" => GUIDE_STOCKS,
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
