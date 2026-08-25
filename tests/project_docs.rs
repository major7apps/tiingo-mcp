use std::{
    collections::BTreeSet,
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
        "## Audited but excluded or deferred",
    );
    assert_exact_tool_table(
        api_tools,
        &discovered,
        "API_SURFACE.md implemented-tool table",
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
}
