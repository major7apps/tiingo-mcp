use crate::{
    client::query::{DateRange, EodResample, IexResample, IntradayResample, NewsQuery, NewsSort},
    error::TiingoError,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, JsonObject},
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
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForexQuoteArgs {
    pub ticker: String,
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
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompanyMetaArgs {
    pub tickers: String,
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

fn nullable_integer_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": ["integer", "null"]})
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
                .get_daily_fundamentals(&args.ticker, range(args.start_date, args.end_date))
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
        tool_result(self.client.get_company_meta(&args.tickers).await)
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
                .get_dividend_yield(&args.ticker, range(args.start_date, args.end_date))
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
}
