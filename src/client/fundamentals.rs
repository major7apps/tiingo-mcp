use super::{
    TiingoClient,
    query::{DateRange, validate_column_list, validate_path_segment, validate_sort},
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_fundamentals_definitions(&self) -> Result<serde_json::Value, TiingoError> {
        self.get_json(
            "fundamentals definitions",
            "/tiingo/fundamentals/definitions",
            &[],
        )
        .await
    }

    /// Fetch the latest financial statement revisions with legacy omission behavior.
    pub async fn get_financial_statements(
        &self,
        ticker: &str,
        range: DateRange,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_financial_statements_with_options(ticker, range, None, None)
            .await
    }

    /// Fetch statements as reported or revised, with optional date-only ordering.
    pub async fn get_financial_statements_with_options(
        &self,
        ticker: &str,
        range: DateRange,
        as_reported: Option<bool>,
        sort: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        if sort.is_some_and(|value| !matches!(value, "date" | "-date")) {
            return Err(TiingoError::Validation(
                "financial statement sort must be date or -date".into(),
            ));
        }
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = as_reported {
            query.push(("asReported", value.to_string()));
        }
        if let Some(value) = validate_sort(sort)? {
            query.push(("sort", value));
        }
        self.get_json(
            "financial statements",
            &format!("/tiingo/fundamentals/{ticker}/statements"),
            &query,
        )
        .await
    }

    /// Fetch daily fundamentals with existing column controls and no ordering override.
    pub async fn get_daily_fundamentals(
        &self,
        ticker: &str,
        range: DateRange,
        columns: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_daily_fundamentals_with_options(ticker, range, columns, None)
            .await
    }

    /// Fetch daily fundamental metrics with validated columns and field ordering.
    pub async fn get_daily_fundamentals_with_options(
        &self,
        ticker: &str,
        range: DateRange,
        columns: Option<&[String]>,
        sort: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
        if let Some(value) = validate_sort(sort)? {
            query.push(("sort", value));
        }
        self.get_json(
            "daily fundamentals",
            &format!("/tiingo/fundamentals/{ticker}/daily"),
            &query,
        )
        .await
    }

    pub async fn get_company_meta(
        &self,
        tickers: &str,
        columns: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        if tickers.is_empty() {
            return Err(TiingoError::Validation("tickers cannot be empty".into()));
        }
        let mut query = vec![("tickers", tickers.to_owned())];
        if let Some(value) = validate_column_list(columns)? {
            query.push(("columns", value));
        }
        self.get_json("company metadata", "/tiingo/fundamentals/meta", &query)
            .await
    }
}
