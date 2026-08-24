use std::future::Future;

use chrono::NaiveDate;
use serde_json::Value;
use tiingo_mcp::{
    client::{
        TiingoClient,
        query::{DateRange, NewsQuery},
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
        }
    }
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
        ResponseShape::NonEmptyObjectArray,
        client.get_forex_quote("eurusd"),
    )
    .await?;
    classify(
        "filtered crypto prices",
        ResponseShape::NonEmptyObjectArray,
        client.get_crypto_quote(Some("btcusd")),
    )
    .await?;
    classify(
        "filtered news",
        ResponseShape::NonEmptyObjectArray,
        client.get_news(NewsQuery {
            tickers: Some("AAPL".to_owned()),
            limit: Some(1),
            ..NewsQuery::default()
        }),
    )
    .await?;
    classify(
        "fundamentals definitions",
        ResponseShape::NonEmptyObjectArray,
        client.get_fundamentals_definitions(),
    )
    .await?;
    classify(
        "corporate-action dividends",
        ResponseShape::NonEmptyObjectArray,
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
