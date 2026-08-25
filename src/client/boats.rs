use super::{
    TiingoClient,
    query::{DateRange, IntradayResample, validate_column_list, validate_path_segment},
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_boats_snapshot(
        &self,
        ticker: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        let path = match ticker {
            Some(ticker) => {
                validate_path_segment(ticker)?;
                format!("/boats/{ticker}")
            }
            None => "/boats".to_owned(),
        };
        self.get_json("BOATS real-time snapshot", &path, &[]).await
    }

    pub async fn get_boats_prices(
        &self,
        ticker: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
        after_hours: Option<bool>,
        columns: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        if let Some(value) = after_hours {
            query.push(("afterHours", value.to_string()));
        }
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
        self.get_json("BOATS prices", &format!("/boats/{ticker}/prices"), &query)
            .await
    }
}
