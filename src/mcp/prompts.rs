use rmcp::{
    ErrorData,
    model::{GetPromptRequestParams, GetPromptResult, Prompt, PromptArgument, PromptMessage, Role},
};
use serde_json::{Map, Value};

fn argument(name: &str, description: &str, required: bool) -> PromptArgument {
    PromptArgument::new(name)
        .with_description(description)
        .with_required(required)
}

pub fn list() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "analyze-stock",
            Some("Comprehensive single-stock analysis: metadata, prices, fundamentals, and news"),
            Some(vec![
                argument("ticker", "Stock ticker symbol", true),
                argument(
                    "include_news",
                    "Whether to include recent news; defaults to true",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "compare-stocks",
            Some("Side-by-side comparison of two stocks: prices, fundamentals, and performance"),
            Some(vec![
                argument("ticker1", "First stock ticker", true),
                argument("ticker2", "Second stock ticker", true),
                argument("period", "Comparison period; defaults to 3 months", false),
            ]),
        ),
        Prompt::new(
            "crypto-market-overview",
            Some("Crypto market snapshot: current prices, 24h changes, and 7-day trends"),
            Some(vec![argument(
                "tickers",
                "Comma-separated crypto tickers; defaults to btcusd,ethusd,solusd",
                false,
            )]),
        ),
        Prompt::new(
            "earnings-report-analysis",
            Some(
                "Analyze a stock's earnings report: financials, price reaction, and news sentiment",
            ),
            Some(vec![
                argument("ticker", "Stock ticker symbol", true),
                argument("earnings_date", "Earnings date in YYYY-MM-DD format", true),
            ]),
        ),
        Prompt::new(
            "forex-pair-analysis",
            Some("Currency pair analysis: current rate, historical trend, and volatility"),
            Some(vec![
                argument("pair", "Lowercase currency pair", true),
                argument("period", "Analysis period; defaults to 1 month", false),
            ]),
        ),
    ]
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
    let text = match request.name.as_str() {
        "analyze-stock" => {
            let ticker = required_string(&arguments, "ticker")?;
            let include_news = optional_bool(&arguments, "include_news", true)?;
            let news = if include_news {
                format!(
                    "5. Call get_news with tickers={ticker} to fetch recent news articles and identify reported catalysts and market-moving events.\n"
                )
            } else {
                "5. Skip news fetching because include_news is false.\n".to_owned()
            };
            format!(
                "Please perform a comprehensive analysis of {ticker} using these steps:\n\n\
                 1. Call get_stock_metadata for {ticker} to retrieve company name, exchange, description, and available date range.\n\
                 2. Call get_company_meta with tickers={ticker} to retrieve sector and industry.\n\
                 3. Call get_stock_prices for {ticker} with a start_date 30 days ago to get recent end-of-day OHLCV history.\n\
                 4. Call get_daily_fundamentals for {ticker} with a start_date 30 days ago to retrieve valuation metrics.\n\
                 {news}\
                 Synthesize the results into these sections:\n\
                 - **Company Overview**: Name, exchange, sector, industry, and business description.\n\
                 - **Price Trend**: Recent price action, highs/lows, and percentage change over 30 days.\n\
                 - **Valuation Snapshot**: Current P/E ratio, market cap, and notable fundamental metrics.\n\
                 - **Recent Catalysts**: Reported news or events if news was fetched; do not infer causes absent evidence.\n\
                 - **Summary**: One-paragraph narrative combining the retrieved findings."
            )
        }
        "compare-stocks" => {
            let ticker1 = required_string(&arguments, "ticker1")?;
            let ticker2 = required_string(&arguments, "ticker2")?;
            let period = optional_string(&arguments, "period", "3 months")?;
            format!(
                "Please compare {ticker1} and {ticker2} over the past {period}.\n\n\
                 1. Call get_stock_prices for both tickers covering the period.\n\
                 2. Call get_daily_fundamentals for both tickers to retrieve P/E ratios and market caps.\n\
                 3. Call get_dividend_yield for both tickers.\n\n\
                 Report a comparison table for price performance, P/E, market cap, and dividend yield; then discuss momentum, relative valuation, income, and a clearly qualified conclusion based only on the retrieved data."
            )
        }
        "crypto-market-overview" => {
            let tickers = optional_string(&arguments, "tickers", "btcusd,ethusd,solusd")?;
            format!(
                "Provide a crypto market overview for {tickers}.\n\n\
                 1. Call get_crypto_quote with tickers={tickers} for current prices and available current fields.\n\
                 2. Call get_crypto_prices for {tickers} with a start_date 7 days ago for trend analysis.\n\n\
                 Report **Current Prices**, **24h Change** when supported by returned observations, **7-Day Trend**, and a **Market Narrative** grounded in the retrieved price data."
            )
        }
        "earnings-report-analysis" => {
            let ticker = required_string(&arguments, "ticker")?;
            let earnings_date = required_string(&arguments, "earnings_date")?;
            format!(
                "Analyze the earnings report for {ticker} around {earnings_date}.\n\n\
                 1. Call get_financial_statements for {ticker} across approximately three months before and after {earnings_date}.\n\
                 2. Call get_stock_prices from two weeks before through two weeks after {earnings_date}.\n\
                 3. Call get_news with tickers={ticker} from one week before through one week after {earnings_date}.\n\n\
                 Report **Financial Results**, **Price Reaction**, **News Sentiment**, and **Outlook**. Expectations data is not available from this server, so do not label the results a beat or miss unless an article supplies an explicit consensus comparison."
            )
        }
        "forex-pair-analysis" => {
            let pair = required_string(&arguments, "pair")?;
            let period = optional_string(&arguments, "period", "1 month")?;
            format!(
                "Analyze {pair} over the past {period}.\n\n\
                 1. Call get_forex_quote for {pair} for the current bid, ask, and mid price.\n\
                 2. Call get_forex_prices for {pair} with a start_date {period} ago and resample_freq='1day'.\n\n\
                 Report **Current Rate**, **Trend over {period}**, **Volatility**, **Notable Moves**, and **Summary**. Describe statistically notable spikes or drops, but state that price data alone cannot establish their cause."
            )
        }
        _ => return Err(ErrorData::invalid_params("prompt not found", None)),
    };
    Ok(GetPromptResult::new(vec![PromptMessage::new_text(
        Role::User,
        text,
    )]))
}
