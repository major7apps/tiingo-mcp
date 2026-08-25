use crate::{
    client::query::{DateRange, EodResample, IexResample, IntradayResample, NewsQuery, NewsSort},
    error::TiingoError,
    websocket::{
        protocol::Service,
        registry::{StartRequest, UpdateRequest},
    },
};
use rmcp::{
    RoleServer,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, JsonObject},
    service::RequestContext,
};
use std::sync::Arc;

use super::TiingoServer;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockMetadataArgs {
    pub ticker: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockPricesArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<EodResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("properties" = {}))]
pub struct BulkEodPricesArgs {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TickerMetadataArgs {
    pub columns: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RealtimePriceArgs {
    pub ticker: String,
    #[serde(default)]
    pub after_hours: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntradayPricesArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<IexResample>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("properties" = {}))]
pub struct IexMarketSnapshotArgs {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EquityRealtimeSnapshotArgs {
    #[serde(default)]
    pub ticker: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EquityIntradayPricesArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<IntradayResample>,
    #[serde(default)]
    pub after_hours: Option<bool>,
    #[serde(default)]
    pub force_fill: Option<bool>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoatsSnapshotArgs {
    #[serde(default)]
    pub ticker: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoatsPricesArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<IntradayResample>,
    #[serde(default)]
    pub after_hours: Option<bool>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FundMetadataArgs {
    pub ticker: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FundFeeMetricsArgs {
    pub ticker: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchTiingoAssetsArgs {
    pub query: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoYieldPlatformsArgs {
    #[serde(default)]
    pub platform_codes: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoYieldPoolsArgs {
    #[serde(default)]
    pub pool_codes: Option<Vec<String>>,
    #[serde(default)]
    pub platform_codes: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoYieldTicksArgs {
    #[serde(default)]
    pub pool_codes: Option<Vec<String>>,
    #[serde(default)]
    pub platform_codes: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoYieldMetricsArgs {
    pub pool_code: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<IntradayResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForexQuoteArgs {
    pub ticker: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForexQuotesArgs {
    pub tickers: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForexPricesArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<IntradayResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoQuoteArgs {
    #[serde(default)]
    pub tickers: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoPricesArgs {
    pub tickers: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub resample_freq: Option<IntradayResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoMetadataArgs {
    #[serde(default)]
    pub tickers: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewsArgs {
    #[serde(default)]
    pub tickers: Option<String>,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(schema_with = "nullable_integer_schema")]
    pub limit: Option<u32>,
    #[serde(default)]
    #[schemars(schema_with = "nullable_integer_schema")]
    pub offset: Option<u32>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub sort_by: Option<NewsSort>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("properties" = {}))]
pub struct FundamentalsDefinitionsArgs {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinancialStatementsArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DailyFundamentalsArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompanyMetaArgs {
    pub tickers: String,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DistributionsByExDateArgs {
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub ex_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DividendsArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DividendYieldArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitsArgs {
    pub ticker: String,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitsByExDateArgs {
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub ex_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MarketDataService {
    Iex,
    Consolidated,
}

impl From<MarketDataService> for Service {
    fn from(service: MarketDataService) -> Self {
        match service {
            MarketDataService::Iex => Self::Iex,
            MarketDataService::Consolidated => Self::Consolidated,
        }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartMarketDataSubscriptionArgs {
    #[schemars(schema_with = "market_data_service_schema")]
    pub service: MarketDataService,
    pub symbols: Vec<String>,
    #[serde(default)]
    pub threshold_level: Option<u8>,
    #[serde(default)]
    pub confirm_iex_market_data_agreement: bool,
}

const fn default_market_data_poll_limit() -> usize {
    256
}

const fn default_market_data_poll_wait_ms() -> u64 {
    5_000
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PollMarketDataSubscriptionArgs {
    pub subscription_id: String,
    #[serde(default)]
    pub after_sequence: u64,
    #[serde(default = "default_market_data_poll_limit")]
    #[schemars(range(max = 256))]
    pub limit: usize,
    #[serde(default = "default_market_data_poll_wait_ms")]
    #[schemars(range(max = 5000))]
    pub max_wait_ms: u64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMarketDataSubscriptionArgs {
    pub subscription_id: String,
    #[serde(default)]
    pub add_symbols: Vec<String>,
    #[serde(default)]
    pub remove_symbols: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopMarketDataSubscriptionArgs {
    pub subscription_id: String,
}

fn nullable_integer_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": ["integer", "null"]})
}

fn market_data_service_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"enum": ["iex", "consolidated"], "type": "string"})
}

fn structured_output_schema() -> Arc<JsonObject> {
    Arc::new(
        serde_json::json!({
            "additionalProperties": false,
            "properties": {
                "data": {},
                "meta": {
                    "additionalProperties": false,
                    "properties": {"source": {"type": "string"}},
                    "required": ["source"],
                    "type": "object"
                }
            },
            "required": ["data", "meta"],
            "type": "object"
        })
        .as_object()
        .expect("structured output schema is an object")
        .clone(),
    )
}

fn success_result(value: serde_json::Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&value).expect("JSON value serializes");
    let mut result = CallToolResult::success(vec![rmcp::model::ContentBlock::text(text)]);
    result.structured_content = Some(serde_json::json!({
        "data": value,
        "meta": { "source": "tiingo" }
    }));
    result
}

fn error_result(error: TiingoError) -> CallToolResult {
    let payload = error.payload();
    let text = serde_json::to_string_pretty(&payload).expect("error payload serializes");
    let mut result = CallToolResult::error(vec![rmcp::model::ContentBlock::text(text)]);
    result.structured_content = Some(serde_json::json!({ "error": payload }));
    result
}

fn tool_result(response: Result<serde_json::Value, TiingoError>) -> CallToolResult {
    match response {
        Ok(value) => success_result(value),
        Err(error) => error_result(error),
    }
}

fn serializable_tool_result<T: serde::Serialize>(
    response: Result<T, TiingoError>,
) -> CallToolResult {
    tool_result(
        response.map(|value| {
            serde_json::to_value(value).expect("market-data lifecycle result serializes")
        }),
    )
}

fn range(start_date: Option<chrono::NaiveDate>, end_date: Option<chrono::NaiveDate>) -> DateRange {
    DateRange {
        start_date,
        end_date,
    }
}

pub(crate) fn tool_router() -> ToolRouter<TiingoServer> {
    let mut router = TiingoServer::tool_router();
    for route in router.map.values_mut() {
        Arc::make_mut(&mut route.attr.input_schema).remove("$schema");
        route.attr.meta = Some(super::compatibility_descriptor_meta());
    }
    router
}

#[rmcp::tool_router(router = tool_router)]
impl TiingoServer {
    #[rmcp::tool(
        description = "Get metadata for a stock ticker including name, exchange, description, and date range.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL, MSFT, GOOGL).",
        output_schema = structured_output_schema()
    )]
    async fn get_stock_metadata(
        &self,
        Parameters(args): Parameters<StockMetadataArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_stock_metadata(&args.ticker).await)
    }

    #[rmcp::tool(
        description = "Get historical end-of-day stock prices with adjusted and unadjusted OHLCV data.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — daily, weekly, monthly, or annually.",
        output_schema = structured_output_schema()
    )]
    async fn get_stock_prices(
        &self,
        Parameters(args): Parameters<StockPricesArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_stock_prices(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get all available end-of-day prices for daily cache refresh. Returns raw and adjusted OHLCV plus historyRefreshTickers for cash dividends or splits.",
        output_schema = structured_output_schema()
    )]
    async fn get_bulk_eod_prices(
        &self,
        Parameters(_args): Parameters<BulkEodPricesArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_bulk_eod_prices().await)
    }

    #[rmcp::tool(
        description = "Get selected ticker lifecycle metadata. The vendor-supplied route is availability-dependent; a 404 means Tiingo did not make this route available for the current request.",
        output_schema = structured_output_schema()
    )]
    async fn get_ticker_metadata(
        &self,
        Parameters(args): Parameters<TickerMetadataArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_ticker_metadata(&args.columns).await)
    }

    #[rmcp::tool(
        description = "Get the current real-time IEX top-of-book price for a stock.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    after_hours: Include after-hours pricing data.",
        output_schema = structured_output_schema()
    )]
    async fn get_realtime_price(
        &self,
        Parameters(args): Parameters<RealtimePriceArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_realtime_price(&args.ticker, args.after_hours)
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get historical intraday prices from IEX at various intervals.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — 1min, 5min, 15min, 30min, 1hour, etc.",
        output_schema = structured_output_schema()
    )]
    async fn get_intraday_prices(
        &self,
        Parameters(args): Parameters<IntradayPricesArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_intraday_prices(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                    args.columns.as_deref(),
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get the all-market IEX snapshot. This bulk route can return a large, entitlement-dependent response.",
        output_schema = structured_output_schema()
    )]
    async fn get_iex_market_snapshot(
        &self,
        Parameters(_args): Parameters<IexMarketSnapshotArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_iex_market_snapshot().await)
    }

    #[rmcp::tool(
        description = "Get a consolidated equity beta snapshot for Tiingo's 4am–8pm ET session. It is distinct from the BOATS beta/add-on 8pm–3:59am ET session; the tools are not a unified 24x5 endpoint. Omit ticker for the all-market snapshot.\n\nArgs:\n    ticker: Optional stock ticker symbol (e.g. AAPL).",
        output_schema = structured_output_schema()
    )]
    async fn get_equity_realtime_snapshot(
        &self,
        Parameters(args): Parameters<EquityRealtimeSnapshotArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_equity_realtime_snapshot(args.ticker.as_deref())
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get consolidated equity beta intraday history for Tiingo's 4am–8pm ET session. It is distinct from the BOATS beta/add-on 8pm–3:59am ET session; the tools are not a unified 24x5 endpoint.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.\n    after_hours: Include after-hours pricing data.\n    force_fill: Forward-fill missing intervals.\n    columns: Optional response column identifiers.",
        output_schema = structured_output_schema()
    )]
    async fn get_equity_intraday_prices(
        &self,
        Parameters(args): Parameters<EquityIntradayPricesArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_equity_intraday_prices(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                    args.after_hours,
                    args.force_fill,
                    args.columns.as_deref(),
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get a BOATS beta/add-on snapshot for Tiingo's 8pm–3:59am ET session. It is distinct from the consolidated equity beta 4am–8pm ET session; the tools are not a unified 24x5 endpoint. Omit ticker for the all-market snapshot.\n\nArgs:\n    ticker: Optional stock ticker symbol (e.g. AAPL).",
        output_schema = structured_output_schema()
    )]
    async fn get_boats_snapshot(
        &self,
        Parameters(args): Parameters<BoatsSnapshotArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_boats_snapshot(args.ticker.as_deref()).await)
    }

    #[rmcp::tool(
        description = "Get BOATS beta/add-on intraday history for Tiingo's 8pm–3:59am ET session. It is distinct from the consolidated equity beta 4am–8pm ET session; the tools are not a unified 24x5 endpoint.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.\n    after_hours: Include after-hours pricing data.\n    columns: Optional response column identifiers.",
        output_schema = structured_output_schema()
    )]
    async fn get_boats_prices(
        &self,
        Parameters(args): Parameters<BoatsPricesArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_boats_prices(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                    args.after_hours,
                    args.columns.as_deref(),
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get mutual-fund or ETF metadata. Fund-fee data is enterprise/institutional access and may return an entitlement error.\n\nArgs:\n    ticker: Fund ticker symbol (e.g. VFIAX).",
        output_schema = structured_output_schema()
    )]
    async fn get_fund_metadata(
        &self,
        Parameters(args): Parameters<FundMetadataArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_fund_metadata(&args.ticker).await)
    }

    #[rmcp::tool(
        description = "Get current and historical mutual-fund or ETF fee metrics. This enterprise/institutional capability may return an entitlement error.\n\nArgs:\n    ticker: Fund ticker symbol (e.g. VFIAX).",
        output_schema = structured_output_schema()
    )]
    async fn get_fund_fee_metrics(
        &self,
        Parameters(args): Parameters<FundFeeMetricsArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_fund_fee_metrics(&args.ticker).await)
    }

    #[rmcp::tool(
        description = "Search Tiingo assets by ticker or name. This endpoint is early beta and its response fields can change.\n\nArgs:\n    query: Nonblank search text, up to 256 characters.",
        output_schema = structured_output_schema()
    )]
    async fn search_tiingo_assets(
        &self,
        Parameters(args): Parameters<SearchTiingoAssetsArgs>,
    ) -> CallToolResult {
        tool_result(self.client.search_tiingo_assets(&args.query).await)
    }

    #[rmcp::tool(
        description = "Get Crypto Yield lending platforms. Crypto Yield access is plan/entitlement-dependent. Omit platform_codes for the full platform list.\n\nArgs:\n    platform_codes: Optional ordered platform-code filters.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_yield_platforms(
        &self,
        Parameters(args): Parameters<CryptoYieldPlatformsArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_crypto_yield_platforms(args.platform_codes.as_deref())
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get Crypto Yield lending-pool metadata. Crypto Yield access is plan/entitlement-dependent. Omit filters for the full pool list.\n\nArgs:\n    pool_codes: Optional ordered pool-code filters.\n    platform_codes: Optional ordered platform-code filters.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_yield_pools(
        &self,
        Parameters(args): Parameters<CryptoYieldPoolsArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_crypto_yield_pools(args.pool_codes.as_deref(), args.platform_codes.as_deref())
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get latest Crypto Yield lending-pool metric ticks. Crypto Yield access is plan/entitlement-dependent. Omit filters for the full tick list.\n\nArgs:\n    pool_codes: Optional ordered pool-code filters.\n    platform_codes: Optional ordered platform-code filters.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_yield_ticks(
        &self,
        Parameters(args): Parameters<CryptoYieldTicksArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_crypto_yield_ticks(args.pool_codes.as_deref(), args.platform_codes.as_deref())
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get historical Crypto Yield OHLC metrics for one pool. Crypto Yield access is plan/entitlement-dependent.\n\nArgs:\n    pool_code: Yield-pool code (e.g. aavev2_usdc).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_yield_metrics(
        &self,
        Parameters(args): Parameters<CryptoYieldMetricsArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_crypto_yield_metrics(
                    &args.pool_code,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get the current top-of-book forex quote for a currency pair.\n\nArgs:\n    ticker: Currency pair (e.g. eurusd, gbpusd, usdjpy).",
        output_schema = structured_output_schema()
    )]
    async fn get_forex_quote(
        &self,
        Parameters(args): Parameters<ForexQuoteArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_forex_quote(&args.ticker).await)
    }

    #[rmcp::tool(
        description = "Get current top-of-book forex quotes for one to 100 currency pairs.\n\nArgs:\n    tickers: Currency pairs to retrieve (e.g. eurusd, gbpusd).",
        output_schema = structured_output_schema()
    )]
    async fn get_forex_quotes(
        &self,
        Parameters(args): Parameters<ForexQuotesArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_forex_quotes(&args.tickers).await)
    }

    #[rmcp::tool(
        description = "Get historical forex prices for a currency pair.\n\nArgs:\n    ticker: Currency pair (e.g. eurusd, gbpusd, usdjpy).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.",
        output_schema = structured_output_schema()
    )]
    async fn get_forex_prices(
        &self,
        Parameters(args): Parameters<ForexPricesArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_forex_prices(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get current crypto prices.\n\nReturns data for all supported cryptos if no tickers specified.\n\nArgs:\n    tickers: Comma-separated crypto tickers (e.g. btcusd, ethusd). Omit for all.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_quote(
        &self,
        Parameters(args): Parameters<CryptoQuoteArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_crypto_quote(args.tickers.as_deref()).await)
    }

    #[rmcp::tool(
        description = "Get historical crypto prices.\n\nArgs:\n    tickers: Comma-separated crypto tickers (e.g. btcusd, ethusd).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    resample_freq: Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_prices(
        &self,
        Parameters(args): Parameters<CryptoPricesArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_crypto_prices(
                    &args.tickers,
                    range(args.start_date, args.end_date),
                    args.resample_freq,
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get metadata for crypto tickers including supported exchanges and pairs.\n\nArgs:\n    tickers: Comma-separated crypto tickers to filter by. Omit for all.",
        output_schema = structured_output_schema()
    )]
    async fn get_crypto_metadata(
        &self,
        Parameters(args): Parameters<CryptoMetadataArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_crypto_metadata(args.tickers.as_deref())
                .await,
        )
    }

    #[rmcp::tool(
        description = "Search financial news articles from 50M+ sources.\n\nArgs:\n    tickers: Comma-separated ticker symbols to filter by (e.g. AAPL,MSFT).\n    tags: Comma-separated tags to filter by.\n    source: News source to filter by.\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.\n    limit: Maximum number of articles to return (default 10).\n    offset: Number of articles to skip for pagination.\n    sort_by: Sort order — crawlDate or publishedDate.",
        output_schema = structured_output_schema()
    )]
    async fn get_news(&self, Parameters(args): Parameters<NewsArgs>) -> CallToolResult {
        tool_result(
            self.client
                .get_news(NewsQuery {
                    tickers: args.tickers,
                    tags: args.tags,
                    source: args.source,
                    start_date: args.start_date,
                    end_date: args.end_date,
                    limit: args.limit,
                    offset: args.offset,
                    sort_by: args.sort_by,
                })
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get definitions for all available fundamental metrics.\n\nReturns the list of metrics available in daily and statement endpoints,\nincluding their names, descriptions, and data types.",
        output_schema = structured_output_schema()
    )]
    async fn get_fundamentals_definitions(
        &self,
        Parameters(_args): Parameters<FundamentalsDefinitionsArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_fundamentals_definitions().await)
    }

    #[rmcp::tool(
        description = "Get quarterly and annual financial statements (income, balance sheet, cash flow).\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.",
        output_schema = structured_output_schema()
    )]
    async fn get_financial_statements(
        &self,
        Parameters(args): Parameters<FinancialStatementsArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_financial_statements(&args.ticker, range(args.start_date, args.end_date))
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get daily fundamental metrics for a stock (market cap, P/E ratio, etc).\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.",
        output_schema = structured_output_schema()
    )]
    async fn get_daily_fundamentals(
        &self,
        Parameters(args): Parameters<DailyFundamentalsArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_daily_fundamentals(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.columns.as_deref(),
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get company metadata including sector, industry, and location.\n\nArgs:\n    tickers: Comma-separated ticker symbols (e.g. AAPL,MSFT,GOOGL).",
        output_schema = structured_output_schema()
    )]
    async fn get_company_meta(
        &self,
        Parameters(args): Parameters<CompanyMetaArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_company_meta(&args.tickers, args.columns.as_deref())
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get distributions across tickers for an optional ex-date. Results may include announced future distributions.\n\nArgs:\n    ex_date: Filter by exact ex-date (YYYY-MM-DD).",
        output_schema = structured_output_schema()
    )]
    async fn get_distributions_by_ex_date(
        &self,
        Parameters(args): Parameters<DistributionsByExDateArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_distributions_by_ex_date(args.ex_date).await)
    }

    #[rmcp::tool(
        description = "Get historical dividend and distribution data for a stock or ETF.\n\nArgs:\n    ticker: Stock/ETF ticker symbol (e.g. AAPL, SPY).\n    start_date: Filter dividends with ex-date on or after this date (YYYY-MM-DD).\n    end_date: Filter dividends with ex-date on or before this date (YYYY-MM-DD).",
        output_schema = structured_output_schema()
    )]
    async fn get_dividends(&self, Parameters(args): Parameters<DividendsArgs>) -> CallToolResult {
        tool_result(
            self.client
                .get_dividends(&args.ticker, range(args.start_date, args.end_date))
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get historical dividend yield data for a stock or ETF.\n\nArgs:\n    ticker: Stock/ETF ticker symbol (e.g. AAPL, SPY).\n    start_date: Start date in YYYY-MM-DD format.\n    end_date: End date in YYYY-MM-DD format.",
        output_schema = structured_output_schema()
    )]
    async fn get_dividend_yield(
        &self,
        Parameters(args): Parameters<DividendYieldArgs>,
    ) -> CallToolResult {
        tool_result(
            self.client
                .get_dividend_yield(
                    &args.ticker,
                    range(args.start_date, args.end_date),
                    args.columns.as_deref(),
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get historical stock split data.\n\nArgs:\n    ticker: Stock ticker symbol (e.g. AAPL, TSLA).\n    start_date: Filter splits with ex-date on or after this date (YYYY-MM-DD).\n    end_date: Filter splits with ex-date on or before this date (YYYY-MM-DD).",
        output_schema = structured_output_schema()
    )]
    async fn get_splits(&self, Parameters(args): Parameters<SplitsArgs>) -> CallToolResult {
        tool_result(
            self.client
                .get_splits(&args.ticker, range(args.start_date, args.end_date))
                .await,
        )
    }

    #[rmcp::tool(
        description = "Get splits across tickers for an optional ex-date. Results may include announced or cancelled future splits.\n\nArgs:\n    ex_date: Filter by exact ex-date (YYYY-MM-DD).",
        output_schema = structured_output_schema()
    )]
    async fn get_splits_by_ex_date(
        &self,
        Parameters(args): Parameters<SplitsByExDateArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_splits_by_ex_date(args.ex_date).await)
    }

    #[rmcp::tool(
        description = "Start a bounded IEX or consolidated equity market-data subscription.",
        output_schema = structured_output_schema()
    )]
    async fn start_market_data_subscription(
        &self,
        Parameters(args): Parameters<StartMarketDataSubscriptionArgs>,
        context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let start = self.market_data.start(StartRequest {
            service: args.service.into(),
            symbols: args.symbols,
            threshold_level: args.threshold_level,
            confirm_iex_market_data_agreement: args.confirm_iex_market_data_agreement,
        });
        tokio::select! {
            result = start => serializable_tool_result(result),
            _ = context.ct.cancelled() => error_result(TiingoError::Validation(
                "the market-data start request was cancelled".into(),
            )),
        }
    }

    #[rmcp::tool(
        description = "Poll a bounded market-data subscription by local arrival sequence.",
        output_schema = structured_output_schema()
    )]
    async fn poll_market_data_subscription(
        &self,
        Parameters(args): Parameters<PollMarketDataSubscriptionArgs>,
        context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let poll = self.market_data.poll_with_bounds(
            &args.subscription_id,
            args.after_sequence,
            args.limit,
            std::time::Duration::from_millis(args.max_wait_ms),
        );
        tokio::select! {
            result = poll => serializable_tool_result(result),
            _ = context.ct.cancelled() => error_result(TiingoError::Validation(
                "the market-data poll request was cancelled".into(),
            )),
        }
    }

    #[rmcp::tool(
        description = "Add or remove symbols on an active market-data subscription.",
        output_schema = structured_output_schema()
    )]
    async fn update_market_data_subscription(
        &self,
        Parameters(args): Parameters<UpdateMarketDataSubscriptionArgs>,
    ) -> CallToolResult {
        serializable_tool_result(
            self.market_data
                .update(
                    &args.subscription_id,
                    UpdateRequest {
                        add_symbols: args.add_symbols,
                        remove_symbols: args.remove_symbols,
                        threshold_level: None,
                    },
                )
                .await,
        )
    }

    #[rmcp::tool(
        description = "Stop a market-data subscription and await worker cleanup.",
        output_schema = structured_output_schema()
    )]
    async fn stop_market_data_subscription(
        &self,
        Parameters(args): Parameters<StopMarketDataSubscriptionArgs>,
    ) -> CallToolResult {
        serializable_tool_result(self.market_data.stop(&args.subscription_id).await)
    }
}
