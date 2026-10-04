use super::{
    TiingoClient,
    query::{
        DateRange, IntradayResample, normalize_symbol_list, validate_column_list,
        validate_path_segment,
    },
};
use crate::error::TiingoError;

impl TiingoClient {
    /// Fetch one consolidated equity ticker, or preserve the all-market default when omitted.
    pub async fn get_equity_realtime_snapshot(
        &self,
        ticker: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_equity_realtime_snapshot_with_tickers(ticker, None)
            .await
    }

    /// Fetch consolidated snapshots with mutually exclusive single-ticker and bounded ticker-list filters.
    pub async fn get_equity_realtime_snapshot_with_tickers(
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
                format!("/tiingo/equity/intraday/{ticker}")
            }
            None => "/tiingo/equity/intraday".to_owned(),
        };
        let mut query = Vec::new();
        if let Some(tickers) = tickers {
            query.push(("tickers", normalize_symbol_list(tickers)?));
        }
        self.get_json("consolidated equity real-time snapshot", &path, &query)
            .await
    }

    /// Fetch consolidated equity bars with validated market intervals and optional request controls.
    pub async fn get_equity_intraday_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
        after_hours: Option<bool>,
        force_fill: Option<bool>,
        columns: Option<&[String]>,
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
        if let Some(value) = force_fill {
            query.push(("forceFill", value.to_string()));
        }
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
        self.get_json(
            "consolidated equity intraday prices",
            &format!("/tiingo/equity/intraday/{ticker}/prices"),
            &query,
        )
        .await
    }
}
