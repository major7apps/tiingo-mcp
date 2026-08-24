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

async fn classify(
    capability: &str,
    request: impl Future<Output = Result<Value, TiingoError>>,
) -> anyhow::Result<(LiveOutcome, Option<Value>)> {
    match request.await {
        Ok(value) => {
            println!("LIVE {capability}: success");
            Ok((LiveOutcome::Success, Some(value)))
        }
        Err(TiingoError::Entitlement { .. }) => {
            println!("LIVE {capability}: entitlement (HTTP 403)");
            Ok((LiveOutcome::Entitlement, None))
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

#[tokio::test]
#[ignore = "requires TIINGO_API_KEY and consumes quota"]
async fn live_read_only_tiingo_capabilities() -> anyhow::Result<()> {
    if std::env::var("TIINGO_API_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        println!("SKIP live smoke: TIINGO_API_KEY is not set");
        return Ok(());
    }

    let client = TiingoClient::from_env()?;

    let (metadata_outcome, metadata) =
        classify("stock metadata", client.get_stock_metadata("AAPL")).await?;
    anyhow::ensure!(
        metadata_outcome == LiveOutcome::Success,
        "stock metadata requires baseline access"
    );
    anyhow::ensure!(
        metadata.is_some_and(|value| value.as_object().is_some_and(|object| !object.is_empty())),
        "stock metadata returned no usable object"
    );

    let (eod_outcome, eod) = classify(
        "short stock EOD range",
        client.get_stock_prices("AAPL", short_range(), None),
    )
    .await?;
    anyhow::ensure!(
        eod_outcome == LiveOutcome::Success,
        "short stock EOD range requires baseline access"
    );
    anyhow::ensure!(
        eod.is_some_and(|value| value.as_array().is_some_and(|rows| !rows.is_empty())),
        "short stock EOD range returned no rows"
    );

    classify("forex pair", client.get_forex_quote("eurusd")).await?;
    classify(
        "filtered crypto prices",
        client.get_crypto_quote(Some("btcusd")),
    )
    .await?;
    classify(
        "filtered news",
        client.get_news(NewsQuery {
            tickers: Some("AAPL".to_owned()),
            limit: Some(1),
            ..NewsQuery::default()
        }),
    )
    .await?;
    classify(
        "fundamentals definitions",
        client.get_fundamentals_definitions(),
    )
    .await?;
    classify(
        "corporate-action dividends",
        client.get_dividends("AAPL", short_range()),
    )
    .await?;

    Ok(())
}
