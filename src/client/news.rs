use super::{
    TiingoClient,
    query::{DateRange, NewsQuery},
};
use crate::error::TiingoError;

impl TiingoClient {
    pub async fn get_news(&self, values: NewsQuery) -> Result<serde_json::Value, TiingoError> {
        let mut query = Vec::new();
        for (name, value) in [
            ("tickers", values.tickers),
            ("tags", values.tags),
            ("source", values.source),
        ] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                query.push((name, value));
            }
        }
        DateRange {
            start_date: values.start_date,
            end_date: values.end_date,
        }
        .append(&mut query);
        if let Some(value) = values.limit {
            query.push(("limit", value.to_string()));
        }
        if let Some(value) = values.offset {
            query.push(("offset", value.to_string()));
        }
        if let Some(value) = values.sort_by {
            query.push(("sortBy", value.as_str().to_owned()));
        }
        self.get_json("news", "/tiingo/news", &query).await
    }
}
