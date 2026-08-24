use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "tiingo-mcp",
    version,
    about = "Tiingo financial-data MCP server"
)]
struct Cli {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
        .ok();
    tiingo_mcp::run_stdio().await
}
