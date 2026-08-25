use super::{TiingoClient, query::validate_path_segment};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_fund_metadata(&self, ticker: &str) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json("fund metadata", &format!("/tiingo/funds/{ticker}"), &[])
            .await
    }

    pub async fn get_fund_fee_metrics(
        &self,
        ticker: &str,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(ticker)?;
        self.get_json(
            "fund fee metrics",
            &format!("/tiingo/funds/{ticker}/metrics"),
            &[],
        )
        .await
    }
}
