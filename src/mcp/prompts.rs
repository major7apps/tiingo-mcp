use rmcp::{
    ErrorData,
    model::{GetPromptRequestParams, GetPromptResult, Prompt, PromptArgument, PromptMessage, Role},
};
use serde_json::{Map, Value};

use super::compatibility_descriptor_meta;

const STRING_ARGUMENT_DESCRIPTION: &str =
    "Provide as a JSON string matching the following schema: {\"type\":\"string\"}";
const BOOLEAN_ARGUMENT_DESCRIPTION: &str =
    "Provide as a JSON string matching the following schema: {\"type\":\"boolean\"}";

fn argument(name: &str, description: &str, required: bool) -> PromptArgument {
    PromptArgument::new(name)
        .with_description(description)
        .with_required(required)
}

pub fn list() -> Vec<Prompt> {
    let mut prompts = vec![
        Prompt::new(
            "analyze-stock",
            Some("Comprehensive single-stock analysis: metadata, prices, fundamentals, and news"),
            Some(vec![
                argument("ticker", STRING_ARGUMENT_DESCRIPTION, true),
                argument("include_news", BOOLEAN_ARGUMENT_DESCRIPTION, false),
            ]),
        ),
        Prompt::new(
            "compare-stocks",
            Some("Side-by-side comparison of two stocks: prices, fundamentals, and performance"),
            Some(vec![
                argument("ticker1", STRING_ARGUMENT_DESCRIPTION, true),
                argument("ticker2", STRING_ARGUMENT_DESCRIPTION, true),
                argument("period", STRING_ARGUMENT_DESCRIPTION, false),
            ]),
        ),
        Prompt::new(
            "crypto-market-overview",
            Some("Crypto market snapshot: current prices, 24h changes, and 7-day trends"),
            Some(vec![argument(
                "tickers",
                STRING_ARGUMENT_DESCRIPTION,
                false,
            )]),
        ),
        Prompt::new(
            "earnings-report-analysis",
            Some(
                "Analyze a stock's earnings report: financials, price reaction, and news sentiment",
            ),
            Some(vec![
                argument("ticker", STRING_ARGUMENT_DESCRIPTION, true),
                argument("earnings_date", STRING_ARGUMENT_DESCRIPTION, true),
            ]),
        ),
        Prompt::new(
            "forex-pair-analysis",
            Some("Currency pair analysis: current rate, historical trend, and volatility"),
            Some(vec![
                argument("pair", STRING_ARGUMENT_DESCRIPTION, true),
                argument("period", STRING_ARGUMENT_DESCRIPTION, false),
            ]),
        ),
    ];
    for prompt in &mut prompts {
        prompt.meta = Some(compatibility_descriptor_meta());
    }
    prompts
}

fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String, ErrorData> {
    match arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        Some(value) => Ok(value.to_owned()),
        None => Err(ErrorData::invalid_params(
            format!("{name} must be a non-empty string"),
            None,
        )),
    }
}

fn optional_string(
    arguments: &Map<String, Value>,
    name: &str,
    default: &str,
) -> Result<String, ErrorData> {
    match arguments.get(name) {
        None => Ok(default.to_owned()),
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(ErrorData::invalid_params(
            format!("{name} must be a non-empty string"),
            None,
        )),
    }
}

fn optional_bool(
    arguments: &Map<String, Value>,
    name: &str,
    default: bool,
) -> Result<bool, ErrorData> {
    match arguments.get(name) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::String(value)) if value == "true" => Ok(true),
        Some(Value::String(value)) if value == "false" => Ok(false),
        _ => Err(ErrorData::invalid_params(
            format!("{name} must be true or false"),
            None,
        )),
    }
}

pub fn get(request: GetPromptRequestParams) -> Result<GetPromptResult, ErrorData> {
    let arguments = request.arguments.unwrap_or_default();
    let (description, text) = match request.name.as_str() {
        "analyze-stock" => {
            let ticker = required_string(&arguments, "ticker")?;
            let include_news = optional_bool(&arguments, "include_news", true)?;
            let news = if include_news {
                format!(
                    "5. Call get_news with tickers={ticker} to fetch recent news articles and identify key catalysts, analyst commentary, and market-moving events.\n"
                )
            } else {
                "5. Skip news fetching because include_news is false.\n".to_owned()
            };
            (
                "Comprehensive single-stock analysis: metadata, prices, fundamentals, and news",
                format!(
                    "Please perform a comprehensive analysis of {ticker} using the following steps:\n\n\
                 1. Call get_stock_metadata for {ticker} to retrieve company name, exchange, description, and available date range.\n\
                 2. Call get_company_meta with tickers={ticker} to retrieve sector and industry.\n\
                 3. Call get_stock_prices for {ticker} with a start_date 30 days ago to get recent end-of-day OHLCV price history.\n\
                 4. Call get_daily_fundamentals for {ticker} with a start_date 30 days ago to retrieve P/E ratio, market cap, and other valuation metrics.\n\
                 {news}\n\
                 Synthesize the results into a structured report with these sections:\n\
                 - **Company Overview**: Name, exchange, sector, and business description.\n\
                 - **Price Trend**: Recent price action, highs/lows, and percentage change over 30 days.\n\
                 - **Valuation Snapshot**: Current P/E ratio, market cap, and notable fundamental metrics.\n\
                 - **Recent Catalysts**: Key news stories or events driving price movement (if news was fetched).\n\
                 - **Summary**: One-paragraph investment narrative combining all findings."
                ),
            )
        }
        "compare-stocks" => {
            let ticker1 = required_string(&arguments, "ticker1")?;
            let ticker2 = required_string(&arguments, "ticker2")?;
            let period = optional_string(&arguments, "period", "3 months")?;
            (
                "Side-by-side comparison of two stocks: prices, fundamentals, and performance",
                format!(
                    "Please perform a side-by-side comparison of {ticker1} and {ticker2} over the past {period}.\n\n\
                 Fetch the following data for each ticker:\n\
                 1. Call get_stock_prices for {ticker1} and get_stock_prices for {ticker2} covering the past {period} to compare price performance.\n\
                 2. Call get_daily_fundamentals for {ticker1} and get_daily_fundamentals for {ticker2} to retrieve P/E ratios and market caps.\n\
                 3. Call get_dividend_yield for {ticker1} and get_dividend_yield for {ticker2} to compare dividend income.\n\n\
                 Produce a comparative analysis including:\n\
                 - A table comparing: price performance (%), P/E ratio, market cap, and dividend yield.\n\
                 - Which of {ticker1} or {ticker2} has stronger momentum over the period.\n\
                 - Relative valuation: which appears cheaper on a P/E basis.\n\
                 - Income comparison: dividend yield difference.\n\
                 - A brief recommendation on which stock looks more attractive and why."
                ),
            )
        }
        "crypto-market-overview" => {
            let tickers = optional_string(&arguments, "tickers", "btcusd,ethusd,solusd")?;
            (
                "Crypto market snapshot: current prices, 24h changes, and 7-day trends",
                format!(
                    "Please provide a crypto market overview for the following tickers: {tickers}.\n\n\
                 Fetch the following data:\n\
                 1. Call get_crypto_quote with tickers={tickers} to get current prices, 24-hour volume, and latest bid/ask.\n\
                 2. Call get_crypto_prices for {tickers} with a start_date 7 days ago to retrieve 7-day price history for trend analysis.\n\n\
                 Summarize the results as a market snapshot:\n\
                 - **Current Prices**: Latest price for each ticker.\n\
                 - **24h Change**: Estimated price change over the past 24 hours based on available data.\n\
                 - **7-Day Trend**: Direction (up/down/flat) and percentage change for each ticker over the past 7 days.\n\
                 - **Market Narrative**: A brief paragraph on overall crypto market sentiment based on the data."
                ),
            )
        }
        "earnings-report-analysis" => {
            let ticker = required_string(&arguments, "ticker")?;
            let earnings_date = required_string(&arguments, "earnings_date")?;
            (
                "Analyze a stock's earnings report: financials, price reaction, and news sentiment",
                format!(
                    "Please analyze the earnings report for {ticker} around the date {earnings_date}.\n\n\
                 Fetch the following data:\n\
                 1. Call get_financial_statements for {ticker} with a date range spanning approximately 3 months before and after {earnings_date} to retrieve the relevant quarterly income statement, balance sheet, and cash flow data.\n\
                 2. Call get_stock_prices for {ticker} with a start_date 2 weeks before {earnings_date} and end_date 2 weeks after {earnings_date} to capture the price reaction around the earnings event.\n\
                 3. Call get_news with tickers={ticker} and a date range 1 week before and after {earnings_date} to gather analyst reactions, guidance commentary, and post-earnings sentiment.\n\n\
                 Synthesize the findings into:\n\
                 - **Financial Results**: Key metrics from the earnings report (revenue, net income, EPS, margins).\n\
                 - **Expectations Context**: Do not label the result a beat or miss unless an article supplies an explicit consensus comparison.\n\
                 - **Price Reaction**: How the stock moved in the 2 weeks before and after earnings.\n\
                 - **News Sentiment**: Summary of analyst and media reaction from the news data.\n\
                 - **Outlook**: Any forward guidance or notable commentary from the news articles."
                ),
            )
        }
        "forex-pair-analysis" => {
            let pair = required_string(&arguments, "pair")?;
            let period = optional_string(&arguments, "period", "1 month")?;
            (
                "Currency pair analysis: current rate, historical trend, and volatility",
                format!(
                    "Please perform a currency pair analysis for {pair} over the past {period}.\n\n\
                 Fetch the following data:\n\
                 1. Call get_forex_quote for {pair} to get the current top-of-book bid, ask, and mid price.\n\
                 2. Call get_forex_prices for {pair} with a start_date {period} ago and resample_freq='1day' to retrieve daily OHLCV history for the period.\n\n\
                 Analyze and present:\n\
                 - **Current Rate**: Latest bid/ask spread and mid price for the pair.\n\
                 - **Trend over {period}**: Direction and magnitude of the rate change, identifying key support/resistance levels.\n\
                 - **Volatility**: Daily price range analysis — average true range or high-low spread over the period.\n\
                 - **Notable Moves**: Identify significant spikes or drops, but state that price history alone cannot establish their cause.\n\
                 - **Summary**: One-paragraph assessment of the pair's current momentum and near-term outlook."
                ),
            )
        }
        _ => return Err(ErrorData::invalid_params("prompt not found", None)),
    };
    Ok(
        GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
            .with_description(description),
    )
}
