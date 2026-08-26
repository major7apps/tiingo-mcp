use super::{
    TiingoClient,
    query::{DateRange, IntradayResample, normalize_symbol_list, validate_path_segment},
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_forex_quotes(
        &self,
        tickers: &[String],
    ) -> Result<serde_json::Value, TiingoError> {
        let tickers = normalize_symbol_list(tickers)?;
        self.get_json("forex quotes", "/tiingo/fx/top", &[("tickers", tickers)])
            .await
    }

    pub async fn get_forex_quote(&self, ticker: &str) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json("forex quote", &format!("/tiingo/fx/{ticker}/top"), &[])
            .await
    }

    pub async fn get_forex_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json(
            "forex prices",
            &format!("/tiingo/fx/{ticker}/prices"),
            &query,
        )
        .await
    }
}
