pub mod client;
pub mod config;
pub mod error;
pub mod mcp;
pub mod websocket;

use rmcp::{ServiceExt, transport::stdio};

pub async fn run_stdio() -> anyhow::Result<()> {
    let service = match mcp::TiingoServer::from_env()?.serve(stdio()).await {
        Ok(service) => service,
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    service.waiting().await?;
    Ok(())
}
