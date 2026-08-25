use super::{
    TiingoClient,
    query::{DateRange, IntradayResample},
};
use crate::error::TiingoError;

fn tickers_query(tickers: Option<&str>) -> Vec<(&'static str, String)> {
    tickers
        .filter(|value| !value.is_empty())
        .map(|value| vec![("tickers", value.to_owned())])
        .unwrap_or_default()
}

impl TiingoClient {
    pub async fn get_crypto_quote(
        &self,
        tickers: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_json(
            "current crypto prices",
            "/tiingo/crypto/prices",
            &tickers_query(tickers),
        )
        .await
    }

    pub async fn get_crypto_prices(
        &self,
        tickers: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        if tickers.is_empty() {
            return Err(TiingoError::Validation("tickers cannot be empty".into()));
        }
        let mut query = vec![("tickers", tickers.to_owned())];
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json("crypto prices", "/tiingo/crypto/prices", &query)
            .await
    }

    pub async fn get_crypto_metadata(
        &self,
        tickers: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_json("crypto metadata", "/tiingo/crypto", &tickers_query(tickers))
            .await
    }
}
