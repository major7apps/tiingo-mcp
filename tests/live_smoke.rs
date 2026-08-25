use std::future::Future;

use chrono::NaiveDate;
use serde_json::Value;
use tiingo_mcp::{
    client::{
        TiingoClient,
        query::{DateRange, IntradayResample, NewsQuery},
    },
    error::TiingoError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveOutcome {
    Success,
    Entitlement,
}

#[derive(Debug, Clone, Copy)]
enum ResponseShape {
    NonEmptyObject,
    NonEmptyObjectArray,
    ForexQuote { ticker: &'static str },
    CryptoQuote { ticker: &'static str },
    NewsArticle,
    FundamentalsDefinition,
    Dividend { ticker: &'static str },
}

impl ResponseShape {
    fn matches(self, value: &Value) -> bool {
        match self {
            Self::NonEmptyObject => value.as_object().is_some_and(|object| !object.is_empty()),
            Self::NonEmptyObjectArray => value.as_array().is_some_and(|rows| {
                !rows.is_empty()
                    && rows
                        .iter()
                        .all(|row| row.as_object().is_some_and(|object| !object.is_empty()))
            }),
            Self::ForexQuote { ticker } => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        string_field_is(object, "ticker", ticker)
                            && has_number(object, &["bidPrice", "askPrice", "midPrice"])
                    })
                })
            }),
            Self::CryptoQuote { ticker } => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        string_field_is(object, "ticker", ticker)
                            && (has_number(
                                object,
                                &["lastPrice", "bidPrice", "askPrice", "midPrice", "close"],
                            ) || nested_rows_have_number(
                                object,
                                "topOfBookData",
                                &["lastPrice", "bidPrice", "askPrice", "midPrice"],
                            ) || nested_rows_have_number(
                                object,
                                "priceData",
                                &["open", "high", "low", "close", "lastPrice"],
                            ))
                    })
                })
            }),
            Self::NewsArticle => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        object.get("id").is_some_and(|id| {
                            id.as_str().is_some_and(|value| !value.is_empty()) || id.is_number()
                        }) && non_empty_string(object, "title")
                            && (non_empty_string(object, "publishedDate")
                                || non_empty_string(object, "crawlDate"))
                    })
                })
            }),
            Self::FundamentalsDefinition => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        ["dataCode", "code", "name"]
                            .iter()
                            .any(|field| non_empty_string(object, field))
                    })
                })
            }),
            Self::Dividend { ticker } => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        string_field_is(object, "ticker", ticker)
                            && non_empty_string(object, "exDate")
                            && has_number(object, &["distribution"])
                    })
                })
            }),
        }
    }
}

fn non_empty_string(object: &serde_json::Map<String, Value>, field: &str) -> bool {
    object
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

fn string_field_is(object: &serde_json::Map<String, Value>, field: &str, expected: &str) -> bool {
    object.get(field).and_then(Value::as_str) == Some(expected)
}

fn has_number(object: &serde_json::Map<String, Value>, fields: &[&str]) -> bool {
    fields
        .iter()
        .any(|field| object.get(*field).is_some_and(Value::is_number))
}

fn nested_rows_have_number(
    object: &serde_json::Map<String, Value>,
    field: &str,
    price_fields: &[&str],
) -> bool {
    object
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.as_object()
                    .is_some_and(|row| has_number(row, price_fields))
            })
        })
}

async fn classify(
    capability: &str,
    expected_shape: ResponseShape,
    request: impl Future<Output = Result<Value, TiingoError>>,
) -> anyhow::Result<LiveOutcome> {
    match request.await {
        Ok(value) => {
            anyhow::ensure!(
                expected_shape.matches(&value),
                "LIVE {capability}: success response was not a representative {expected_shape:?}"
            );
            println!("LIVE {capability}: success");
            Ok(LiveOutcome::Success)
        }
        Err(TiingoError::Entitlement { .. }) => {
            println!("LIVE {capability}: entitlement (HTTP 403)");
            Ok(LiveOutcome::Entitlement)
        }
        Err(error) => Err(anyhow::anyhow!("LIVE {capability}: {error}")),
    }
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn short_range() -> DateRange {
    DateRange {
        start_date: Some(date(2024, 1, 2)),
        end_date: Some(date(2024, 1, 5)),
    }
}

fn corporate_action_range() -> DateRange {
    DateRange {
        start_date: Some(date(2023, 1, 1)),
        end_date: Some(date(2024, 12, 31)),
    }
}

fn require_live_api_key() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var("TIINGO_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some(),
        "TIINGO_API_KEY must be set to a non-empty value when an ignored live smoke is selected"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY and consumes quota"]
async fn live_consolidated_equity_single_ticker() -> anyhow::Result<()> {
    require_live_api_key()?;
    let client = TiingoClient::from_env()?;

    classify(
        "consolidated equity snapshot",
        ResponseShape::NonEmptyObjectArray,
        client.get_equity_realtime_snapshot(Some("AAPL")),
    )
    .await?;
    classify(
        "consolidated equity intraday prices",
        ResponseShape::NonEmptyObjectArray,
        client.get_equity_intraday_prices(
            "AAPL",
            DateRange::default(),
            Some(IntradayResample::OneHour),
            None,
            None,
            None,
        ),
    )
    .await?;

    Ok(())
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY and consumes quota"]
async fn live_boats_single_ticker() -> anyhow::Result<()> {
    require_live_api_key()?;
    let client = TiingoClient::from_env()?;

    classify(
        "BOATS snapshot",
        ResponseShape::NonEmptyObjectArray,
        client.get_boats_snapshot(Some("AAPL")),
    )
    .await?;
    classify(
        "BOATS prices",
        ResponseShape::NonEmptyObjectArray,
        client.get_boats_prices(
            "AAPL",
            DateRange::default(),
            Some(IntradayResample::OneHour),
            None,
            None,
        ),
    )
    .await?;

    Ok(())
}

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY and consumes quota"]
async fn live_read_only_tiingo_capabilities() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var("TIINGO_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some(),
        "TIINGO_API_KEY must be set to a non-empty value when the ignored live smoke is selected"
    );

    let client = TiingoClient::from_env()?;

    let metadata_outcome = classify(
        "stock metadata",
        ResponseShape::NonEmptyObject,
        client.get_stock_metadata("AAPL"),
    )
    .await?;
    anyhow::ensure!(
        metadata_outcome == LiveOutcome::Success,
        "stock metadata requires baseline access"
    );

    let eod_outcome = classify(
        "short stock EOD range",
        ResponseShape::NonEmptyObjectArray,
        client.get_stock_prices("AAPL", short_range(), None),
    )
    .await?;
    anyhow::ensure!(
        eod_outcome == LiveOutcome::Success,
        "short stock EOD range requires baseline access"
    );

    classify(
        "forex pair",
        ResponseShape::ForexQuote { ticker: "eurusd" },
        client.get_forex_quote("eurusd"),
    )
    .await?;
    classify(
        "filtered crypto quote",
        ResponseShape::CryptoQuote { ticker: "btcusd" },
        client.get_crypto_quote(Some("btcusd")),
    )
    .await?;
    classify(
        "filtered news",
        ResponseShape::NewsArticle,
        client.get_news(NewsQuery {
            tickers: Some("AAPL".to_owned()),
            limit: Some(1),
            ..NewsQuery::default()
        }),
    )
    .await?;
    classify(
        "fundamentals definitions",
        ResponseShape::FundamentalsDefinition,
        client.get_fundamentals_definitions(),
    )
    .await?;
    classify(
        "corporate-action dividends",
        ResponseShape::Dividend { ticker: "AAPL" },
        client.get_dividends("AAPL", corporate_action_range()),
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn empty_success_is_not_live_capability_evidence() {
    let error = classify(
        "empty example",
        ResponseShape::NonEmptyObjectArray,
        std::future::ready(Ok(serde_json::json!([]))),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("not a representative"));
}

#[tokio::test]
async fn wrong_family_object_is_not_live_capability_evidence() {
    let error = classify(
        "forex pair",
        ResponseShape::ForexQuote { ticker: "eurusd" },
        std::future::ready(Ok(serde_json::json!([{"error": "wrong route"}]))),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("not a representative"));
}

#[test]
fn family_specific_live_shapes_require_identity_and_stable_fields() {
    let forex = ResponseShape::ForexQuote { ticker: "eurusd" };
    assert!(forex.matches(&serde_json::json!([{
        "ticker": "eurusd",
        "midPrice": 1.08
    }])));
    assert!(!forex.matches(&serde_json::json!([{
        "ticker": "gbpusd",
        "midPrice": 1.27
    }])));
    assert!(!forex.matches(&serde_json::json!([{"ticker": "eurusd"}])));

    let crypto = ResponseShape::CryptoQuote { ticker: "btcusd" };
    assert!(crypto.matches(&serde_json::json!([{
        "ticker": "btcusd",
        "priceData": [{"close": 64000.0}]
    }])));
    assert!(!crypto.matches(&serde_json::json!([{
        "ticker": "ethusd",
        "priceData": [{"close": 3200.0}]
    }])));
    assert!(!crypto.matches(&serde_json::json!([{"ticker": "btcusd"}])));

    let news = ResponseShape::NewsArticle;
    assert!(news.matches(&serde_json::json!([{
        "id": 42,
        "title": "Apple reports results",
        "publishedDate": "2026-08-24T12:00:00Z"
    }])));
    assert!(!news.matches(&serde_json::json!([{
        "title": "Missing article identity",
        "publishedDate": "2026-08-24T12:00:00Z"
    }])));
    assert!(!news.matches(&serde_json::json!([{
        "id": 42,
        "title": "Missing article date"
    }])));

    let fundamentals = ResponseShape::FundamentalsDefinition;
    assert!(fundamentals.matches(&serde_json::json!([{
        "dataCode": "revenue",
        "description": "Total revenue"
    }])));
    assert!(!fundamentals.matches(&serde_json::json!([{
        "description": "Missing definition discriminator"
    }])));

    let dividend = ResponseShape::Dividend { ticker: "AAPL" };
    assert!(dividend.matches(&serde_json::json!([{
        "ticker": "AAPL",
        "exDate": "2024-11-08",
        "distribution": 0.25
    }])));
    assert!(!dividend.matches(&serde_json::json!([{
        "ticker": "MSFT",
        "exDate": "2024-11-08",
        "distribution": 0.25
    }])));
    assert!(!dividend.matches(&serde_json::json!([{
        "ticker": "AAPL",
        "exDate": "2024-11-08"
    }])));
}
