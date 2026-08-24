use super::{
    TiingoClient,
    query::{DateRange, validate_path_segment},
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

    pub async fn get_financial_statements(
        &self,
        ticker: &str,
        range: DateRange,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        self.get_json(
            "financial statements",
            &format!("/tiingo/fundamentals/{ticker}/statements"),
            &query,
        )
        .await
    }

    pub async fn get_daily_fundamentals(
        &self,
        ticker: &str,
        range: DateRange,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        let mut query = Vec::new();
        range.append(&mut query);
        self.get_json(
            "daily fundamentals",
            &format!("/tiingo/fundamentals/{ticker}/daily"),
            &query,
        )
        .await
    }

    pub async fn get_company_meta(&self, tickers: &str) -> Result<serde_json::Value, TiingoError> {
        if tickers.is_empty() {
            return Err(TiingoError::Validation("tickers cannot be empty".into()));
        }
        self.get_json(
            "company metadata",
            "/tiingo/fundamentals/meta",
            &[("tickers", tickers.to_owned())],
        )
        .await
    }
}
