use super::{
    TiingoClient,
    query::{DateRange, EodResample, validate_path_segment, validate_ticker_metadata_columns},
};
use crate::error::TiingoError;

#[derive(serde::Deserialize, serde::Serialize)]
struct BulkEodPrice {
    date: String,
    ticker: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    #[serde(rename = "adjOpen")]
    adj_open: f64,
    #[serde(rename = "adjHigh")]
    adj_high: f64,
    #[serde(rename = "adjLow")]
    adj_low: f64,
    #[serde(rename = "adjClose")]
    adj_close: f64,
    #[serde(rename = "adjVolume")]
    adj_volume: f64,
    #[serde(rename = "divCash")]
    div_cash: f64,
    #[serde(rename = "splitFactor")]
    split_factor: f64,
}

impl BulkEodPrice {
    fn has_only_finite_financial_fields(&self) -> bool {
        [
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.adj_open,
            self.adj_high,
            self.adj_low,
            self.adj_close,
            self.adj_volume,
            self.div_cash,
            self.split_factor,
        ]
        .into_iter()
        .all(f64::is_finite)
    }
}

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

    pub async fn get_bulk_eod_prices(&self) -> Result<serde_json::Value, TiingoError> {
        let csv = self
            .get_csv(
                "bulk EOD prices",
                "/tiingo/daily/prices",
                &[("format", "csv".to_owned())],
            )
            .await?;
        let mut prices = Vec::new();
        let mut history_refresh_tickers = Vec::new();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(csv.as_bytes());
        let headers = reader.headers().map_err(|_| TiingoError::Decode {
            capability: "bulk EOD prices",
        })?;
        for required in [
            "date",
            "ticker",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "adjOpen",
            "adjHigh",
            "adjLow",
            "adjClose",
            "adjVolume",
            "divCash",
            "splitFactor",
        ] {
            if headers.iter().filter(|header| *header == required).count() != 1 {
                return Err(TiingoError::Decode {
                    capability: "bulk EOD prices",
                });
            }
        }

        for row in reader.deserialize::<BulkEodPrice>() {
            let price = row.map_err(|_| TiingoError::Decode {
                capability: "bulk EOD prices",
            })?;
            if price.date.trim().is_empty()
                || price.ticker.trim().is_empty()
                || !price.has_only_finite_financial_fields()
            {
                return Err(TiingoError::Decode {
                    capability: "bulk EOD prices",
                });
            }
            if price.split_factor != 1.0 || price.div_cash > 0.0 {
                history_refresh_tickers.push(price.ticker.clone());
            }
            prices.push(
                serde_json::to_value(price).map_err(|_| TiingoError::Decode {
                    capability: "bulk EOD prices",
                })?,
            );
        }

        Ok(serde_json::json!({
            "prices": prices,
            "historyRefreshTickers": history_refresh_tickers,
        }))
    }

    pub async fn get_ticker_metadata(
        &self,
        columns: &[String],
    ) -> Result<serde_json::Value, TiingoError> {
        validate_ticker_metadata_columns(columns)?;
        self.get_json(
            "ticker metadata",
            "/tiingo/daily/meta",
            &[("columns", columns.join(","))],
        )
        .await
    }
}
