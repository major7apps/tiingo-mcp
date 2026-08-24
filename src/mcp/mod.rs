use std::sync::Arc;

use crate::client::TiingoClient;
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
};

pub mod tools;

#[derive(Clone, Debug)]
pub struct TiingoServer {
    pub(crate) client: Arc<TiingoClient>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl TiingoServer {
    pub fn with_client(client: TiingoClient) -> Self {
        Self {
            client: Arc::new(client),
            tool_router: tools::tool_router(),
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self::with_client(TiingoClient::from_env()?))
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
impl ServerHandler for TiingoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Financial data server powered by Tiingo. Dates use YYYY-MM-DD.")
    }
}
