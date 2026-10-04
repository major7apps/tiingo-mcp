use super::{
    TiingoClient,
    query::{DateRange, IntradayResample, validate_exchange_list},
};
use crate::error::TiingoError;

fn tickers_query(tickers: Option<&str>) -> Vec<(&'static str, String)> {
    tickers
        .filter(|value| !value.is_empty())
        .map(|value| vec![("tickers", value.to_owned())])
        .unwrap_or_default()
}

impl TiingoClient {
    /// Fetch current crypto prices with optional comma-separated ticker filters.
    pub async fn get_crypto_quote(
        &self,
        tickers: Option<&str>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_crypto_quote_with_options(tickers, None, None)
            .await
    }

    /// Fetch crypto quotes with validated interval and exchange filters; preserve omitted defaults.
    pub async fn get_crypto_quote_with_options(
        &self,
        tickers: Option<&str>,
        resample: Option<IntradayResample>,
        exchanges: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        let mut query = tickers_query(tickers);
        if let Some(value) = resample {
            value.validate()?;
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        if let Some(value) = validate_exchange_list(exchanges)? {
            query.push(("exchanges", value));
        }
        self.get_json("current crypto prices", "/tiingo/crypto/prices", &query)
            .await
    }

    /// Fetch historical crypto bars with the legacy omission behavior.
    pub async fn get_crypto_prices(
        &self,
        tickers: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        self.get_crypto_prices_with_options(tickers, range, resample, None)
            .await
    }

    /// Fetch historical crypto bars with validated interval and ordered exchange filters.
    pub async fn get_crypto_prices_with_options(
        &self,
        tickers: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
        exchanges: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        if tickers.is_empty() {
            return Err(TiingoError::Validation("tickers cannot be empty".into()));
        }
        let mut query = vec![("tickers", tickers.to_owned())];
        range.append(&mut query);
        if let Some(value) = resample {
            value.validate()?;
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        if let Some(value) = validate_exchange_list(exchanges)? {
            query.push(("exchanges", value));
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
