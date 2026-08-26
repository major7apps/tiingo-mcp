pub mod client;
pub mod config;
pub mod error;
pub mod mcp;
pub mod websocket;

use rmcp::ServiceExt;

#[doc(hidden)]
pub async fn run_stdio_with<R, W>(
    server: mcp::TiingoServer,
    input: R,
    output: W,
) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let market_data = server.market_data.clone();
    let result = match server.serve((input, output)).await {
        Ok(service) => service.waiting().await.map(|_| ()).map_err(Into::into),
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => Ok(()),
        Err(error) => Err(error.into()),
    };
    market_data.shutdown().await;
    result
}

pub async fn run_stdio() -> anyhow::Result<()> {
    let (input, output) = rmcp::transport::stdio();
    run_stdio_with(mcp::TiingoServer::from_env()?, input, output).await
}
