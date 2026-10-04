use super::{
    TiingoClient,
    query::{
        DateRange, IexResample, normalize_symbol_list, validate_column_list, validate_path_segment,
    },
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_iex_market_snapshot(&self) -> Result<serde_json::Value, TiingoError> {
        self.get_iex_market_snapshot_with_tickers(None).await
    }

    pub async fn get_iex_market_snapshot_with_tickers(
        &self,
        tickers: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        let mut query = Vec::new();
        if let Some(tickers) = tickers {
            query.push(("tickers", normalize_symbol_list(tickers)?));
        }
        self.get_json("IEX market snapshot", "/iex", &query).await
    }

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
        columns: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_intraday_prices_with_options(ticker, range, resample, columns, None, None)
            .await
    }

    pub async fn get_intraday_prices_with_options(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IexResample>,
        columns: Option<&[String]>,
        after_hours: Option<bool>,
        force_fill: Option<bool>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            value.validate()?;
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
        if let Some(value) = after_hours {
            query.push(("afterHours", value.to_string()));
        }
        if let Some(value) = force_fill {
            query.push(("forceFill", value.to_string()));
        }
        self.get_json(
            "intraday stock prices",
            &format!("/iex/{ticker}/prices"),
            &query,
        )
        .await
    }
}
