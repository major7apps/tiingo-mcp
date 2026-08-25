use super::{
    TiingoClient,
    query::{DateRange, IexResample, validate_path_segment},
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_realtime_price(
        &self,
        ticker: &str,
        after_hours: Option<bool>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let query = after_hours
            .map(|value| vec![("afterHours", value.to_string())])
            .unwrap_or_default();
        self.get_json("real-time stock price", &format!("/iex/{ticker}"), &query)
            .await
    }

    pub async fn get_intraday_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IexResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json(
            "intraday stock prices",
            &format!("/iex/{ticker}/prices"),
            &query,
        )
        .await
    }
}
