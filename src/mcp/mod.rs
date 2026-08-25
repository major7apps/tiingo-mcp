use std::sync::Arc;

use crate::client::TiingoClient;
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{
        ExtensionCapabilities, Implementation, JsonObject, MetaObject, ServerCapabilities,
        ServerInfo,
    },
};

pub mod prompts;
pub mod resources;
pub mod tools;

pub(crate) fn compatibility_descriptor_meta() -> MetaObject {
    MetaObject(
        serde_json::json!({"fastmcp": {"tags": []}})
            .as_object()
            .expect("compatibility metadata is an object")
            .clone(),
    )
}

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
    #[allow(deprecated)]
    fn get_info(&self) -> ServerInfo {
        let mut extensions = ExtensionCapabilities::new();
        extensions.insert("io.modelcontextprotocol/ui".to_owned(), JsonObject::new());
        let mut capabilities = ServerCapabilities::builder()
            .enable_experimental()
            .enable_extensions_with(extensions)
            .enable_logging()
            .enable_tools()
            .enable_tool_list_changed()
            .enable_resources()
            .enable_resources_list_changed()
            .enable_prompts()
            .enable_prompts_list_changed()
            .build();
        capabilities
            .resources
            .as_mut()
            .expect("resources enabled")
            .subscribe = Some(false);
        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new(
                "Tiingo MCP Server",
                env!("CARGO_PKG_VERSION"),
            ))
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

    async fn list_prompts(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListPromptsResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListPromptsResult::with_all_items(
            prompts::list(),
        ))
    }

    async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::GetPromptResponse, rmcp::ErrorData> {
        Ok(prompts::get(request)?.into())
    }
}
