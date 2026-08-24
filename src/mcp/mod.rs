use rmcp::{
    ServerHandler,
    model::{ServerCapabilities, ServerInfo},
};

#[derive(Clone, Debug, Default)]
pub struct TiingoServer;

impl TiingoServer {
    pub fn new() -> Self {
        Self
    }
}

impl ServerHandler for TiingoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().build())
            .with_instructions("Financial data server powered by Tiingo. Dates use YYYY-MM-DD.")
    }
}
