use std::sync::Arc;

use crate::client::TiingoClient;
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
};

pub mod resources;
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
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions("Financial data server powered by Tiingo. Dates use YYYY-MM-DD.")
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListResourcesResult::with_all_items(
            resources::list(),
        ))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListResourceTemplatesResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        ))
    }

    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResponse, rmcp::ErrorData> {
        Ok(resources::read(&request.uri)?.into())
    }
}
