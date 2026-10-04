use super::TiingoClient;
use crate::error::TiingoError;

const MAX_SEARCH_QUERY_CHARS: usize = 256;

pub fn validate_search_query(query: &str) -> Result<String, TiingoError> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(TiingoError::Validation(format!(
            "query must contain between 1 and {MAX_SEARCH_QUERY_CHARS} non-whitespace characters"
        )));
    }
    Ok(query.to_owned())
}

impl TiingoClient {
    pub async fn search_tiingo_assets(
        &self,
        query: &str,
    ) -> Result<serde_json::Value, TiingoError> {
        self.search_tiingo_assets_with_options(query, None, None, None)
            .await
    }

    pub async fn search_tiingo_assets_with_options(
        &self,
        query: &str,
        exact_ticker_match: Option<bool>,
        include_delisted: Option<bool>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, TiingoError> {
        let query = validate_search_query(query)?;
        let mut query = vec![("query", query)];
        if let Some(value) = exact_ticker_match {
            query.push(("exactTickerMatch", value.to_string()));
        }
        if let Some(value) = include_delisted {
            query.push(("includeDelisted", value.to_string()));
        }
        if let Some(value) = limit {
            if !(1..=100).contains(&value) {
                return Err(TiingoError::Validation(
                    "limit must be between 1 and 100".into(),
                ));
            }
            query.push(("limit", value.to_string()));
        }
        self.get_json("Tiingo asset search", "/tiingo/utilities/search", &query)
            .await
    }
}
