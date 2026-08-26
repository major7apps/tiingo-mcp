use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rmcp::{RoleClient, ServiceExt, service::RunningService};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    mcp::TiingoServer,
};
use url::Url;

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

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_root_file(name: &str) -> String {
    fs::read_to_string(repository_root().join(name))
        .unwrap_or_else(|error| panic!("root project reference {name} must exist: {error}"))
}

fn markdown_section<'a>(markdown: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = markdown
        .find(start)
        .unwrap_or_else(|| panic!("missing markdown section {start}"));
    let remainder = &markdown[start_index + start.len()..];
    let end = remainder
        .find(end)
        .unwrap_or_else(|| panic!("missing markdown section boundary {end}"));
    &remainder[..end]
}

fn first_column_code_values(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter_map(|line| {
            let first_column = line.strip_prefix('|')?.split('|').next()?.trim();
            first_column
                .strip_prefix('`')?
                .strip_suffix('`')
                .map(str::to_owned)
        })
        .collect()
}

fn markdown_table_rows(markdown: &str) -> Vec<Vec<String>> {
    markdown
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix('|')?.strip_suffix('|')?;
            let cells = line
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect::<Vec<_>>();
            (!cells
                .first()
                .is_some_and(|cell| cell == "Tool" || cell == "Ignored test" || cell == "---"))
            .then_some(cells)
        })
        .collect()
}

fn code_values(value: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut remainder = value;
    while let Some(open) = remainder.find('`') {
        remainder = &remainder[open + 1..];
        let Some(close) = remainder.find('`') else {
            break;
        };
        values.insert(remainder[..close].to_owned());
        remainder = &remainder[close + 1..];
    }
    values
}

fn ignored_test_names() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(repository_root().join("tests")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let mut awaiting_function = false;
        for line in source.lines() {
            let line = line.trim();
            if line.starts_with("#[ignore") {
                awaiting_function = true;
            } else if awaiting_function && let Some(function) = line.strip_prefix("async fn ") {
                names.insert(function.split('(').next().unwrap().to_owned());
                awaiting_function = false;
            }
        }
    }
    names
}

fn assert_exact_tool_table(markdown: &str, discovered: &BTreeSet<String>, label: &str) {
    let listed = first_column_code_values(markdown);
    let unique = listed.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        listed.len(),
        unique.len(),
        "{label} repeats a registered tool"
    );
    assert_eq!(
        unique, *discovered,
        "{label} must list the exact discovered MCP tool surface"
    );
}

fn local_markdown_links(markdown: &str) -> Vec<&str> {
    let mut links = Vec::new();
    let mut remainder = markdown;
    while let Some(open) = remainder.find("](") {
        remainder = &remainder[open + 2..];
        let Some(close) = remainder.find(')') else {
            break;
        };
        let target = &remainder[..close];
        if !target.contains("://") && !target.starts_with('#') {
            links.push(target.split('#').next().unwrap());
        }
        remainder = &remainder[close + 1..];
    }
    links
}

fn json_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(json_files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    files.sort();
    files
}

#[tokio::test]
async fn registered_tools_appear_once_in_each_public_tool_matrix() {
    let connection = Connection::new().await;
    let discovered = connection
        .client
        .list_tools(None)
        .await
        .unwrap()
        .tools
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();

    let readme = read_root_file("README.md");
    let readme_tools = markdown_section(&readme, "## Tools", "## Resources");
    assert_exact_tool_table(readme_tools, &discovered, "README.md tool tables");

    let api_surface = read_root_file("API_SURFACE.md");
    let api_tools = markdown_section(
        &api_surface,
        "## Implemented tools",
        "## Checked-in ignored live smokes",
    );
    assert_exact_tool_table(
        api_tools,
        &discovered,
        "API_SURFACE.md implemented-tool table",
    );

    connection.close().await;
}

#[tokio::test]
async fn api_surface_uses_exact_rows_vocab_sources_and_live_inventory() {
    const BULK_EOD_SOURCE: &str = "https://www.tiingo.com/kb/article/the-fastest-method-to-ingest-tiingo-end-of-day-stock-api-data/";
    const CRYPTO_YIELD_SOURCE: &str = "https://www.tiingo.com/documentation/crypto-yield";

    let connection = Connection::new().await;
    let discovered = connection
        .client
        .list_tools(None)
        .await
        .unwrap()
        .tools
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();
    let api_surface = read_root_file("API_SURFACE.md");
    assert!(
        api_surface.contains("`L` (bounded read-only live-testable)"),
        "L must describe live-testability, not checked-in test coverage"
    );
    assert!(api_surface.contains(BULK_EOD_SOURCE));
    assert!(api_surface.contains(CRYPTO_YIELD_SOURCE));

    let implemented = markdown_section(
        &api_surface,
        "## Implemented tools",
        "## Checked-in ignored live smokes",
    );
    let rows = markdown_table_rows(implemented);
    assert_eq!(rows.len(), 38);
    let approved_statuses = BTreeSet::from(["documented", "beta", "vendor-supplied", "lifecycle"]);
    let approved_classes = BTreeSet::from(["D", "L", "E", "Q"]);
    let mut tools = BTreeMap::new();
    for row in rows {
        assert_eq!(row.len(), 5, "implemented tool rows must have five cells");
        let tool = row[0].trim_matches('`').to_owned();
        assert!(
            discovered.contains(&tool),
            "unknown API surface tool {tool}"
        );
        assert!(
            approved_statuses.contains(row[2].as_str()),
            "{tool} has unapproved status {}",
            row[2]
        );
        let classes = row[4].split(',').map(str::trim).collect::<BTreeSet<_>>();
        assert!(
            classes.contains("D"),
            "{tool} must be deterministic-testable"
        );
        assert!(
            classes.iter().all(|class| approved_classes.contains(class)),
            "{tool} has unapproved test/access vocabulary: {}",
            row[4]
        );
        assert!(tools.insert(tool, row).is_none());
    }
    for tool in [
        "get_distributions_by_ex_date",
        "get_dividends",
        "get_dividend_yield",
        "get_splits",
        "get_splits_by_ex_date",
    ] {
        assert_eq!(tools[tool][2], "beta", "{tool} must be beta/early-release");
        assert!(
            tools[tool][3].contains("Early-release"),
            "{tool} must identify the corporate-action surface as early-release"
        );
    }
    assert_eq!(tools["get_fundamentals_definitions"][4], "D, L, E, Q");

    let inventory = markdown_section(
        &api_surface,
        "## Checked-in ignored live smokes",
        "## Audited but excluded or deferred",
    );
    let inventory = markdown_table_rows(inventory)
        .into_iter()
        .map(|row| {
            assert_eq!(row.len(), 3, "live inventory rows must have three cells");
            (row[0].trim_matches('`').to_owned(), code_values(&row[1]))
        })
        .collect::<BTreeMap<_, _>>();
    let expected = BTreeMap::from([
        (
            "live_boats_single_ticker".to_owned(),
            BTreeSet::from([
                "get_boats_prices".to_owned(),
                "get_boats_snapshot".to_owned(),
            ]),
        ),
        (
            "live_consolidated_equity_single_ticker".to_owned(),
            BTreeSet::from([
                "get_equity_intraday_prices".to_owned(),
                "get_equity_realtime_snapshot".to_owned(),
            ]),
        ),
        (
            "live_consolidated_level_six_single_ticker_websocket".to_owned(),
            BTreeSet::from([
                "poll_market_data_subscription".to_owned(),
                "start_market_data_subscription".to_owned(),
                "stop_market_data_subscription".to_owned(),
            ]),
        ),
        (
            "live_crypto_yield_metrics_single_pool".to_owned(),
            BTreeSet::from(["get_crypto_yield_metrics".to_owned()]),
        ),
        (
            "live_distributions_by_ex_date_tiny_filter".to_owned(),
            BTreeSet::from(["get_distributions_by_ex_date".to_owned()]),
        ),
        (
            "live_forex_quotes_single_pair".to_owned(),
            BTreeSet::from(["get_forex_quotes".to_owned()]),
        ),
        (
            "live_fund_fees_single_ticker".to_owned(),
            BTreeSet::from([
                "get_fund_fee_metrics".to_owned(),
                "get_fund_metadata".to_owned(),
            ]),
        ),
        (
            "live_iex_level_six_single_ticker_websocket".to_owned(),
            BTreeSet::from([
                "poll_market_data_subscription".to_owned(),
                "start_market_data_subscription".to_owned(),
                "stop_market_data_subscription".to_owned(),
            ]),
        ),
        (
            "live_mcp_eod_data_is_consistent_accurate_and_timely".to_owned(),
            BTreeSet::from(["get_stock_prices".to_owned()]),
        ),
        (
            "live_read_only_tiingo_capabilities".to_owned(),
            BTreeSet::from([
                "get_crypto_quote".to_owned(),
                "get_dividends".to_owned(),
                "get_forex_quote".to_owned(),
                "get_fundamentals_definitions".to_owned(),
                "get_news".to_owned(),
                "get_stock_metadata".to_owned(),
                "get_stock_prices".to_owned(),
            ]),
        ),
        (
            "live_search_early_beta".to_owned(),
            BTreeSet::from(["search_tiingo_assets".to_owned()]),
        ),
        (
            "live_splits_by_ex_date_tiny_filter".to_owned(),
            BTreeSet::from(["get_splits_by_ex_date".to_owned()]),
        ),
    ]);
    assert_eq!(inventory, expected);
    assert_eq!(
        inventory.keys().cloned().collect::<BTreeSet<_>>(),
        ignored_test_names(),
        "API live inventory must match checked-in ignored tests"
    );
    assert!(
        inventory
            .values()
            .flatten()
            .all(|tool| discovered.contains(tool))
    );

    connection.close().await;
}

#[tokio::test]
async fn embedded_references_parse_and_match_the_discovered_public_counts() {
    let connection = Connection::new().await;
    let tools = connection.client.list_tools(None).await.unwrap().tools;
    let tool_names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<BTreeSet<_>>();
    assert_eq!(tools.len(), 38);
    assert_eq!(
        connection
            .client
            .list_resources(None)
            .await
            .unwrap()
            .resources
            .len(),
        3
    );
    assert_eq!(
        connection
            .client
            .list_resource_templates(None)
            .await
            .unwrap()
            .resource_templates
            .len(),
        1
    );
    assert_eq!(
        connection
            .client
            .list_prompts(None)
            .await
            .unwrap()
            .prompts
            .len(),
        5
    );

    let data_directory = repository_root().join("src/mcp/data");
    for path in json_files(&data_directory) {
        let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|error| panic!("{} must contain valid JSON: {error}", path.display()));
        if path.parent() == Some(data_directory.join("guides").as_path()) {
            for tool in value["tools"]
                .as_array()
                .unwrap_or_else(|| panic!("{} must declare a tools array", path.display()))
            {
                let tool = tool.as_str().unwrap();
                assert!(
                    tool_names.contains(tool),
                    "{} references undiscoverable tool {tool}",
                    path.display()
                );
            }
        }
    }

    connection.close().await;
}

#[test]
fn root_project_reference_links_resolve_and_claude_uses_the_canonical_guide() {
    let root = repository_root();
    let agents = read_root_file("AGENTS.md");
    for reference in ["ARCHITECTURE.md", "API_SURFACE.md", "QUALITY.md"] {
        assert!(
            agents.contains(&format!("]({reference})")),
            "AGENTS.md must map readers to {reference}"
        );
    }

    for source in [
        "AGENTS.md",
        "README.md",
        "ARCHITECTURE.md",
        "API_SURFACE.md",
        "QUALITY.md",
    ] {
        let markdown = read_root_file(source);
        for link in local_markdown_links(&markdown) {
            assert!(
                Path::new(link).is_absolute() || root.join(link).exists(),
                "{source} has a broken local link: {link}"
            );
        }
    }

    let metadata = fs::symlink_metadata(root.join("CLAUDE.md")).unwrap();
    assert!(
        metadata.file_type().is_symlink(),
        "CLAUDE.md must be a symlink"
    );
    assert_eq!(
        fs::read_link(root.join("CLAUDE.md")).unwrap(),
        Path::new("AGENTS.md")
    );

    assert!(
        agents.contains("four approved optional `columns` additions"),
        "AGENTS.md must state the exact backward-compatible legacy-tool delta"
    );
    let api_surface = read_root_file("API_SURFACE.md");
    assert!(
        api_surface.contains("four approved optional `columns` additions"),
        "API_SURFACE.md must state the exact backward-compatible legacy-tool delta"
    );

    let architecture = read_root_file("ARCHITECTURE.md");
    assert!(
        architecture.contains("This server exposes MCP over stdio only."),
        "ARCHITECTURE.md must scope the transport statement to this server"
    );
    assert!(!architecture.contains("MCP uses stdio only."));

    let readme = read_root_file("README.md");
    assert!(
        readme.contains("`terminalError`"),
        "README.md must document sanitized terminal WebSocket classifications"
    );
    assert!(
        readme.contains("Published 2.0.2 installers and the crates.io package expose the released 17-tool surface"),
        "README.md must distinguish the published release from the unreleased tool expansion"
    );
    assert!(
        readme.contains("The unreleased development tree exposes 38 tools"),
        "README.md must identify the 38-tool surface as unreleased"
    );
    assert!(
        architecture.contains("frame and reassembled-message limits"),
        "ARCHITECTURE.md must document concrete socket-level message bounds"
    );
    assert!(
        architecture.contains("serializes the fixed envelope once"),
        "ARCHITECTURE.md must document linear poll byte admission"
    );

    assert!(
        api_surface.contains("sanitized terminal classification"),
        "API_SURFACE.md must identify terminal poll classification"
    );
    let quality = read_root_file("QUALITY.md");
    assert!(
        quality.contains("`tests/websocket_logging.rs`"),
        "QUALITY.md must map the dependency trace-leak regression"
    );

    let changelog = read_root_file("CHANGELOG.md");
    assert!(
        changelog.contains("socket-level 8-MiB frame/message limits"),
        "CHANGELOG.md must record the public WebSocket hardening"
    );
}
