use super::{
    TiingoClient,
    query::{DateRange, validate_column_list, validate_path_segment},
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

fn batch_ex_date_query(ex_date: Option<chrono::NaiveDate>) -> Vec<(&'static str, String)> {
    ex_date
        .map(|value| vec![("exDate", value.format("%Y-%m-%d").to_string())])
        .unwrap_or_default()
}

impl TiingoClient {
    pub async fn get_distributions_by_ex_date(
        &self,
        ex_date: Option<chrono::NaiveDate>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_json(
            "distributions by ex-date",
            "/tiingo/corporate-actions/distributions",
            &batch_ex_date_query(ex_date),
        )
        .await
    }

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
        columns: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
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

    pub async fn get_splits_by_ex_date(
        &self,
        ex_date: Option<chrono::NaiveDate>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_json(
            "splits by ex-date",
            "/tiingo/corporate-actions/splits",
            &batch_ex_date_query(ex_date),
        )
        .await
    }
}
