use super::{
    TiingoClient,
    query::{DateRange, EodResample, validate_path_segment},
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_stock_metadata(&self, ticker: &str) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json("stock metadata", &format!("/tiingo/daily/{ticker}"), &[])
            .await
    }

    pub async fn get_stock_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<EodResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json(
            "stock prices",
            &format!("/tiingo/daily/{ticker}/prices"),
            &query,
        )
        .await
    }
}
