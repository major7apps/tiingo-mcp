use super::{
    TiingoClient,
    query::{DateRange, IntradayResample, validate_path_segment},
};
use crate::error::TiingoError;

fn code_list(values: Option<&[String]>) -> Result<Option<String>, TiingoError> {
    let Some(values) = values else {
        return Ok(None);
    };
    if values.is_empty() || values.len() > 100 {
        return Err(TiingoError::Validation(
            "codes must contain between 1 and 100 values".to_owned(),
        ));
    }
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        validate_path_segment(value)?;
        normalized.push(value);
    }
    Ok(Some(normalized.join(",")))
}

fn validate_date_order(range: DateRange) -> Result<(), TiingoError> {
    if range
        .start_date
        .zip(range.end_date)
        .is_some_and(|(start, end)| start > end)
    {
        return Err(TiingoError::Validation(
            "start_date must not be after end_date".to_owned(),
        ));
    }
    Ok(())
}

fn yield_filters(
    pool_codes: Option<&[String]>,
    platform_codes: Option<&[String]>,
) -> Result<Vec<(&'static str, String)>, TiingoError> {
    let mut query = Vec::new();
    if let Some(value) = code_list(pool_codes)? {
        query.push(("poolCodes", value));
    }
    if let Some(value) = code_list(platform_codes)? {
        query.push(("platformCodes", value));
    }
    Ok(query)
}

impl TiingoClient {
    pub async fn get_crypto_yield_platforms(
        &self,
        platform_codes: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        let query = code_list(platform_codes)?
            .map(|value| vec![("platformCodes", value)])
            .unwrap_or_default();
        self.get_json(
            "crypto yield platforms",
            "/tiingo/crypto-yield/platforms",
            &query,
        )
        .await
    }

    pub async fn get_crypto_yield_pools(
        &self,
        pool_codes: Option<&[String]>,
        platform_codes: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        let query = yield_filters(pool_codes, platform_codes)?;
        self.get_json("crypto yield pools", "/tiingo/crypto-yield/pools", &query)
            .await
    }

    pub async fn get_crypto_yield_ticks(
        &self,
        pool_codes: Option<&[String]>,
        platform_codes: Option<&[String]>,
    ) -> Result<serde_json::Value, TiingoError> {
        let query = yield_filters(pool_codes, platform_codes)?;
        self.get_json("crypto yield ticks", "/tiingo/crypto-yield/ticks", &query)
            .await
    }

    pub async fn get_crypto_yield_metrics(
        &self,
        pool_code: &str,
        range: DateRange,
        resample: Option<IntradayResample>,
    ) -> Result<serde_json::Value, TiingoError> {
        validate_path_segment(pool_code)?;
        validate_date_order(range)?;
        let mut query = Vec::new();
        range.append(&mut query);
        if let Some(value) = resample {
            query.push(("resampleFreq", value.as_str().to_owned()));
        }
        self.get_json(
            "crypto yield metrics",
            &format!("/tiingo/crypto-yield/{pool_code}/metrics"),
            &query,
        )
        .await
    }
}
