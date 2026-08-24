use crate::{
    client::query::{DateRange, EodResample, IntradayResample, NewsQuery, NewsSort},
    error::TiingoError,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::CallToolResult,
};

use super::TiingoServer;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockMetadataArgs {
    /// Stock ticker symbol (e.g. AAPL, MSFT, GOOGL).
    pub ticker: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockPricesArgs {
    /// Stock ticker symbol (e.g. AAPL).
    pub ticker: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
    /// Resample frequency — daily, weekly, monthly, or annually.
    #[serde(default)]
    pub resample_freq: Option<EodResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RealtimePriceArgs {
    /// Stock ticker symbol (e.g. AAPL).
    pub ticker: String,
    /// Include after-hours pricing data.
    #[serde(default)]
    pub after_hours: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntradayPricesArgs {
    /// Stock ticker symbol (e.g. AAPL).
    pub ticker: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
    /// Resample frequency — 1min, 5min, 15min, 30min, 1hour, etc.
    #[serde(default)]
    pub resample_freq: Option<IntradayResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForexQuoteArgs {
    /// Currency pair (e.g. eurusd, gbpusd, usdjpy).
    pub ticker: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForexPricesArgs {
    /// Currency pair (e.g. eurusd, gbpusd, usdjpy).
    pub ticker: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
    /// Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.
    #[serde(default)]
    pub resample_freq: Option<IntradayResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoQuoteArgs {
    /// Comma-separated crypto tickers (e.g. btcusd, ethusd). Omit for all.
    #[serde(default)]
    pub tickers: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoPricesArgs {
    /// Comma-separated crypto tickers (e.g. btcusd, ethusd).
    pub tickers: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
    /// Resample frequency — 1min, 5min, 15min, 30min, 1hour, 1day.
    #[serde(default)]
    pub resample_freq: Option<IntradayResample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CryptoMetadataArgs {
    /// Comma-separated crypto tickers to filter by. Omit for all.
    #[serde(default)]
    pub tickers: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewsArgs {
    /// Comma-separated ticker symbols to filter by (e.g. AAPL,MSFT).
    #[serde(default)]
    pub tickers: Option<String>,
    /// Comma-separated tags to filter by.
    #[serde(default)]
    pub tags: Option<String>,
    /// News source to filter by.
    #[serde(default)]
    pub source: Option<String>,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
    /// Maximum number of articles to return (default 10).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Number of articles to skip for pagination.
    #[serde(default)]
    pub offset: Option<u32>,
    /// Sort order — crawlDate or publishedDate.
    #[serde(default)]
    pub sort_by: Option<NewsSort>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("properties" = {}))]
pub struct FundamentalsDefinitionsArgs {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinancialStatementsArgs {
    /// Stock ticker symbol (e.g. AAPL).
    pub ticker: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DailyFundamentalsArgs {
    /// Stock ticker symbol (e.g. AAPL).
    pub ticker: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompanyMetaArgs {
    /// Comma-separated ticker symbols (e.g. AAPL,MSFT,GOOGL).
    pub tickers: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DividendsArgs {
    /// Stock/ETF ticker symbol (e.g. AAPL, SPY).
    pub ticker: String,
    /// Filter dividends with ex-date on or after this date (YYYY-MM-DD).
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// Filter dividends with ex-date on or before this date (YYYY-MM-DD).
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DividendYieldArgs {
    /// Stock/ETF ticker symbol (e.g. AAPL, SPY).
    pub ticker: String,
    /// Start date in YYYY-MM-DD format.
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// End date in YYYY-MM-DD format.
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitsArgs {
    /// Stock ticker symbol (e.g. AAPL, TSLA).
    pub ticker: String,
    /// Filter splits with ex-date on or after this date (YYYY-MM-DD).
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    /// Filter splits with ex-date on or before this date (YYYY-MM-DD).
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
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
    TiingoServer::tool_router()
}

#[rmcp::tool_router(router = tool_router)]
impl TiingoServer {
    #[rmcp::tool(
        description = "Get metadata for a stock ticker including name, exchange, description, and date range."
    )]
    async fn get_stock_metadata(
        &self,
        Parameters(args): Parameters<StockMetadataArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_stock_metadata(&args.ticker).await)
    }

    #[rmcp::tool(
        description = "Get historical end-of-day stock prices with adjusted and unadjusted OHLCV data."
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

    #[rmcp::tool(description = "Get the current real-time IEX top-of-book price for a stock.")]
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

    #[rmcp::tool(description = "Get historical intraday prices from IEX at supported intervals.")]
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

    #[rmcp::tool(description = "Get the current top-of-book forex quote for a currency pair.")]
    async fn get_forex_quote(
        &self,
        Parameters(args): Parameters<ForexQuoteArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_forex_quote(&args.ticker).await)
    }

    #[rmcp::tool(description = "Get historical forex prices for a currency pair.")]
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

    #[rmcp::tool(description = "Get current crypto prices, optionally filtered by ticker.")]
    async fn get_crypto_quote(
        &self,
        Parameters(args): Parameters<CryptoQuoteArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_crypto_quote(args.tickers.as_deref()).await)
    }

    #[rmcp::tool(description = "Get historical crypto prices.")]
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
        description = "Get metadata for crypto tickers including supported exchanges and pairs."
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
        description = "Search financial news articles by ticker, tag, source, date, and sort order."
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

    #[rmcp::tool(description = "Get definitions for Tiingo fundamental data fields.")]
    async fn get_fundamentals_definitions(
        &self,
        Parameters(_args): Parameters<FundamentalsDefinitionsArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_fundamentals_definitions().await)
    }

    #[rmcp::tool(description = "Get quarterly and annual financial statements for a company.")]
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
        description = "Get daily fundamental metrics such as market cap and valuation ratios."
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
        description = "Get company metadata including sector, industry, country, and SIC code."
    )]
    async fn get_company_meta(
        &self,
        Parameters(args): Parameters<CompanyMetaArgs>,
    ) -> CallToolResult {
        tool_result(self.client.get_company_meta(&args.tickers).await)
    }

    #[rmcp::tool(description = "Get dividend distribution history for a ticker.")]
    async fn get_dividends(&self, Parameters(args): Parameters<DividendsArgs>) -> CallToolResult {
        tool_result(
            self.client
                .get_dividends(&args.ticker, range(args.start_date, args.end_date))
                .await,
        )
    }

    #[rmcp::tool(description = "Get historical dividend yield for a ticker.")]
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

    #[rmcp::tool(description = "Get stock split history for a ticker.")]
    async fn get_splits(&self, Parameters(args): Parameters<SplitsArgs>) -> CallToolResult {
        tool_result(
            self.client
                .get_splits(&args.ticker, range(args.start_date, args.end_date))
                .await,
        )
    }
}
