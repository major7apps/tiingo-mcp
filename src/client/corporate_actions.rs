use super::{
    TiingoClient,
    query::{DateRange, validate_path_segment},
};
use crate::error::TiingoError;

fn ex_date_query(range: DateRange) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(value) = range.start_date {
        query.push(("startExDate", value.format("%Y-%m-%d").to_string()));
    }
    if let Some(value) = range.end_date {
        query.push(("endExDate", value.format("%Y-%m-%d").to_string()));
    }
    query
}

impl TiingoClient {
    pub async fn get_dividends(
        &self,
        ticker: &str,
        range: DateRange,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json(
            "dividends",
            &format!("/tiingo/corporate-actions/{ticker}/distributions"),
            &ex_date_query(range),
        )
        .await
    }

    pub async fn get_dividend_yield(
        &self,
        ticker: &str,
        range: DateRange,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        self.get_json(
            "dividend yield",
            &format!("/tiingo/corporate-actions/{ticker}/distribution-yield"),
            &query,
        )
        .await
    }

    pub async fn get_splits(
        &self,
        ticker: &str,
        range: DateRange,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json(
            "splits",
            &format!("/tiingo/corporate-actions/{ticker}/splits"),
            &ex_date_query(range),
        )
        .await
    }
}
