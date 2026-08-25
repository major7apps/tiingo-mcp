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
        let query = validate_search_query(query)?;
        self.get_json(
            "Tiingo asset search",
            "/tiingo/utilities/search",
            &[("query", query)],
        )
        .await
    }
}
