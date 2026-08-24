pub mod client;
pub mod config;
pub mod error;
pub mod mcp;

use rmcp::{ServiceExt, transport::stdio};

pub async fn run_stdio() -> anyhow::Result<()> {
    let service = mcp::TiingoServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
