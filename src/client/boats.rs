use super::{
    TiingoClient,
    query::{
        DateRange, IntradayResample, normalize_symbol_list, validate_column_list,
        validate_path_segment,
    },
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_boats_snapshot(
        &self,
        ticker: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_boats_snapshot_with_tickers(ticker, None).await
    }

    pub async fn get_boats_snapshot_with_tickers(
        &self,
        ticker: Option<&str>,
        tickers: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        if ticker.is_some() && tickers.is_some() {
            return Err(TiingoError::Validation(
                "ticker and tickers cannot be supplied together".into(),
            ));
        }
        let path = match ticker {
            Some(ticker) => {
                validate_path_segment(ticker)?;
                format!("/boats/{ticker}")
            }
            None => "/boats".to_owned(),
        };
        let mut query = Vec::new();
        if let Some(tickers) = tickers {
            query.push(("tickers", normalize_symbol_list(tickers)?));
        }
        self.get_json("BOATS real-time snapshot", &path, &query)
            .await
    }

    pub async fn get_boats_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
        after_hours: Option<bool>,
        columns: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_boats_prices_with_options(ticker, range, resample, after_hours, columns, None)
            .await
    }

    pub async fn get_boats_prices_with_options(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
        after_hours: Option<bool>,
        columns: Option<&[String]>,
        force_fill: Option<bool>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            value.validate_market_interval()?;
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        if let Some(value) = after_hours {
            query.push(("afterHours", value.to_string()));
        }
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
        if let Some(value) = force_fill {
            query.push(("forceFill", value.to_string()));
        }
        self.get_json("BOATS prices", &format!("/boats/{ticker}/prices"), &query)
            .await
    }
}
