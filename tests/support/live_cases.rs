use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use tiingo_mcp::websocket::protocol::{MarketData, ProtocolCodec, ServerMessage, Service};
use tiingo_mcp::websocket::registry::{PollResult, SubscriptionStatus, TerminalErrorKind};

// Stable response fields from official Tiingo documentation, reviewed 2026-10-04.
// https://www.tiingo.com/documentation/mutual-fund-and-etf-fees
// https://www.tiingo.com/documentation/utilities/search
// https://www.tiingo.com/documentation/crypto-yield
#[derive(Debug, Clone, Copy)]
pub enum ResponseShape {
    StockMetadata,
    PriceBar,
    CryptoHistory,
    RequiredRows {
        identity: Option<(&'static str, &'static str)>,
        strings: &'static [&'static str],
        numbers: &'static [&'static str],
        objects: &'static [&'static str],
    },
    FundMetadata,
    FundMetrics,
    SearchAsset,
    YieldMetric,
    ForexQuote {
        ticker: &'static str,
    },
    CryptoQuote {
        ticker: &'static str,
    },
    NewsArticle,
    FundamentalsDefinition,
    Dividend {
        ticker: &'static str,
    },
    Split {
        ticker: &'static str,
    },
    EquitySnapshot {
        ticker: &'static str,
        price_fields: &'static [&'static str],
    },
    IntradayBar,
}

impl ResponseShape {
    /// Require the documented response-family fields and identities before accepting live data.
    pub fn matches(self, value: &Value) -> bool {
        match self {
            Self::StockMetadata => value.as_object().is_some_and(|row| {
                string_field_is(row, "ticker", "AAPL") && non_empty_string(row, "name")
            }),
            Self::PriceBar => all_rows(value, |row| {
                non_empty_string(row, "date") && valid_ohlc_bar(row)
            }),
            Self::CryptoHistory => all_rows(value, |row| {
                string_field_is(row, "ticker", "btcusd")
                    && row
                        .get("priceData")
                        .is_some_and(|bars| Self::PriceBar.matches(bars))
            }),
            Self::RequiredRows {
                identity,
                strings,
                numbers,
                objects,
            } => all_rows(value, |row| {
                identity.is_none_or(|(field, expected)| string_field_is(row, field, expected))
                    && strings.iter().all(|field| non_empty_string(row, field))
                    && numbers
                        .iter()
                        .all(|field| row.get(*field).is_some_and(Value::is_number))
                    && objects
                        .iter()
                        .all(|field| row.get(*field).is_some_and(Value::is_object))
            }),
            Self::FundMetadata => value.as_object().is_some_and(|row| {
                string_field_is(row, "ticker", "VFIAX") && non_empty_string(row, "name")
            }),
            Self::FundMetrics => all_rows(value, |row| {
                non_empty_string(row, "prospectusDate") && has_number(row, &["netExpense"])
            }),
            Self::SearchAsset => all_rows(value, |row| {
                non_empty_string(row, "ticker")
                    && non_empty_string(row, "name")
                    && non_empty_string(row, "assetType")
                    && row.get("isActive").is_some_and(Value::is_boolean)
            }),
            Self::YieldMetric => all_rows(value, |row| {
                non_empty_string(row, "date") && has_number(row, &["closeSupplyRate"])
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
                        non_empty_string(object, "dataCode")
                            && object.get("description").is_some_and(Value::is_string)
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
            Self::Split { ticker } => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        string_field_is(object, "ticker", ticker)
                            && non_empty_string(object, "exDate")
                            && has_number(object, &["splitFactor"])
                    })
                })
            }),
            Self::EquitySnapshot {
                ticker,
                price_fields,
            } => value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row.as_object().is_some_and(|object| {
                        string_field_is(object, "ticker", ticker)
                            && has_number(object, price_fields)
                    })
                })
            }),
            Self::IntradayBar => value.as_array().is_some_and(|rows| {
                !rows.is_empty()
                    && rows.iter().all(|row| {
                        row.as_object().is_some_and(|object| {
                            non_empty_string(object, "date") && valid_ohlcv_bar(object)
                        })
                    })
            }),
        }
    }
}

/// Require a nonempty array whose every object satisfies the family predicate.
fn all_rows(value: &Value, predicate: impl Fn(&serde_json::Map<String, Value>) -> bool) -> bool {
    value.as_array().is_some_and(|rows| {
        !rows.is_empty()
            && rows
                .iter()
                .all(|row| row.as_object().is_some_and(&predicate))
    })
}

/// Require a named response field to contain a nonempty string.
fn non_empty_string(object: &serde_json::Map<String, Value>, field: &str) -> bool {
    object
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

/// Compare ticker identities without ASCII case sensitivity and other identifiers exactly.
fn string_field_is(object: &serde_json::Map<String, Value>, field: &str, expected: &str) -> bool {
    object
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|actual| {
            if field == "ticker" {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == expected
            }
        })
}

/// Require at least one documented numeric field in a response object.
fn has_number(object: &serde_json::Map<String, Value>, fields: &[&str]) -> bool {
    fields
        .iter()
        .any(|field| object.get(*field).is_some_and(Value::is_number))
}

/// Require valid OHLC bounds and a nonnegative numeric volume.
fn valid_ohlcv_bar(object: &serde_json::Map<String, Value>) -> bool {
    valid_ohlc_bar(object)
        && object
            .get("volume")
            .and_then(Value::as_f64)
            .is_some_and(|volume| volume >= 0.0)
}

/// Require numeric OHLC values whose high and low bound the open and close.
fn valid_ohlc_bar(object: &serde_json::Map<String, Value>) -> bool {
    let Some(open) = object.get("open").and_then(Value::as_f64) else {
        return false;
    };
    let Some(high) = object.get("high").and_then(Value::as_f64) else {
        return false;
    };
    let Some(low) = object.get("low").and_then(Value::as_f64) else {
        return false;
    };
    let Some(close) = object.get("close").and_then(Value::as_f64) else {
        return false;
    };
    high >= open && high >= close && high >= low && low <= open && low <= close
}

/// Require a nested price array to contain a documented numeric field.
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

#[derive(Debug, PartialEq, Eq)]
pub enum MarketPollOutcome {
    Delivery,
    AcknowledgedNoData,
    Entitlement,
}

/// Classify a bounded IEX/AAPL level-six poll using the shared delivery validator.
pub fn classify_market_poll(poll: &PollResult) -> anyhow::Result<MarketPollOutcome> {
    classify_market_poll_for_service(poll, Service::Iex, &["AAPL"])
}

/// Distinguish delivery, acknowledged empty data, entitlement, and unexpected terminal states.
pub fn classify_market_poll_for_service(
    poll: &PollResult,
    service: Service,
    symbols: &[&str],
) -> anyhow::Result<MarketPollOutcome> {
    if poll.state == SubscriptionStatus::Failed
        && poll.terminal_error == Some(TerminalErrorKind::Entitlement)
    {
        return Ok(MarketPollOutcome::Entitlement);
    }
    anyhow::ensure!(
        poll.state == SubscriptionStatus::Active && poll.terminal_error.is_none(),
        "live poll is not active: state={:?}, terminal={:?}",
        poll.state,
        poll.terminal_error
    );
    anyhow::ensure!(
        poll.events.len() <= 1,
        "bounded poll returned more than one event"
    );
    let mut delivered = false;
    for event in &poll.events {
        delivered |= validate_market_event(&event.payload, service, symbols)?;
    }
    Ok(if delivered {
        MarketPollOutcome::Delivery
    } else {
        MarketPollOutcome::AcknowledgedNoData
    })
}

/// Decode a level-six event and require subscribed identity, RFC3339 time, and a finite price.
pub fn validate_market_event(
    payload: &Value,
    service: Service,
    symbols: &[&str],
) -> anyhow::Result<bool> {
    let codec = ProtocolCodec::new(service, 6)?;
    let decoded = codec.decode(&serde_json::to_vec(payload)?, chrono::Utc::now())?;
    match decoded.message {
        ServerMessage::Market(
            MarketData::IexReference(reference) | MarketData::ConsolidatedReference(reference),
        ) => {
            anyhow::ensure!(
                symbols
                    .iter()
                    .any(|symbol| reference.ticker.eq_ignore_ascii_case(symbol)),
                "market event ticker does not belong to the subscription"
            );
            anyhow::ensure!(
                chrono::DateTime::parse_from_rfc3339(&reference.vendor_timestamp).is_ok(),
                "market event timestamp is not RFC3339"
            );
            anyhow::ensure!(
                reference.reference_price.is_finite(),
                "market event price is not finite"
            );
            Ok(true)
        }
        ServerMessage::Heartbeat(_) => Ok(false),
        _ => anyhow::bail!("bounded poll returned an unexpected message type"),
    }
}

/// Parse an exact calendar date or RFC3339 timestamp without accepting arbitrary suffixes.
fn calendar_date(date: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(date)
                .ok()
                .map(|date| date.date_naive())
        })
}

pub struct RestCase {
    pub tool: &'static str,
    arguments_json: &'static str,
    pub shape: ResponseShape,
}

impl RestCase {
    /// Build bounded case arguments, refreshing the rolling history window where required.
    pub fn arguments(&self) -> Value {
        let mut value: Value = serde_json::from_str(self.arguments_json)
            .expect("literal live arguments are valid JSON");
        if matches!(self.tool, "get_equity_intraday_prices" | "get_boats_prices") {
            let end = chrono::Utc::now().date_naive();
            value["start_date"] = (end - chrono::Duration::days(7)).to_string().into();
            value["end_date"] = end.to_string().into();
        }
        value
    }

    /// Require the response family plus the requested ticker, code, date window, and OHLC invariants.
    pub fn validate(&self, data: &Value, arguments: &Value) -> bool {
        if !self.shape.matches(data) {
            return false;
        }
        let Some(rows) = data.as_array() else {
            return true;
        };
        let date_rows = if self.tool == "get_crypto_prices" {
            rows.iter()
                .flat_map(|row| row["priceData"].as_array().into_iter().flatten())
                .collect::<Vec<_>>()
        } else {
            rows.iter().collect::<Vec<_>>()
        };
        let expected_ex_date = arguments.get("ex_date").and_then(Value::as_str);
        if let Some(expected) = expected_ex_date {
            let expected = calendar_date(expected);
            return expected.is_some()
                && date_rows
                    .iter()
                    .all(|row| row["exDate"].as_str().and_then(calendar_date) == expected);
        }
        let start = arguments.get("start_date").and_then(Value::as_str);
        let end = arguments.get("end_date").and_then(Value::as_str);
        if start.is_none() && end.is_none() {
            return true;
        }
        date_rows.iter().all(|row| {
            let field = if matches!(self.tool, "get_dividends" | "get_splits") {
                "exDate"
            } else {
                "date"
            };
            row[field]
                .as_str()
                .and_then(calendar_date)
                .is_some_and(|date| {
                    start
                        .is_none_or(|start| calendar_date(start).is_some_and(|start| date >= start))
                        && end.is_none_or(|end| calendar_date(end).is_some_and(|end| date <= end))
                })
        })
    }
}

pub const REST_CASES: &[RestCase] = &[
    RestCase {
        tool: "get_stock_metadata",
        arguments_json: r#"{"ticker":"AAPL"}"#,
        shape: ResponseShape::StockMetadata,
    },
    RestCase {
        tool: "get_stock_prices",
        arguments_json: r#"{"ticker":"AAPL","start_date":"2024-01-02","end_date":"2024-01-02"}"#,
        shape: ResponseShape::IntradayBar,
    },
    RestCase {
        tool: "get_realtime_price",
        arguments_json: r#"{"ticker":"AAPL"}"#,
        shape: ResponseShape::EquitySnapshot {
            ticker: "AAPL",
            price_fields: &["tngoLast", "last"],
        },
    },
    RestCase {
        tool: "get_intraday_prices",
        arguments_json: r#"{"ticker":"AAPL","start_date":"2024-01-02","end_date":"2024-01-02","resample_freq":"1hour"}"#,
        shape: ResponseShape::IntradayBar,
    },
    RestCase {
        tool: "get_iex_market_snapshot",
        arguments_json: r#"{"tickers":["AAPL"]}"#,
        shape: ResponseShape::EquitySnapshot {
            ticker: "AAPL",
            price_fields: &["tngoLast", "last"],
        },
    },
    RestCase {
        tool: "get_equity_realtime_snapshot",
        arguments_json: r#"{"ticker":"AAPL"}"#,
        shape: ResponseShape::EquitySnapshot {
            ticker: "AAPL",
            price_fields: &["tngoLast", "lqRefPrice", "prevClose"],
        },
    },
    RestCase {
        tool: "get_equity_intraday_prices",
        arguments_json: r#"{"ticker":"AAPL","resample_freq":"1hour","columns":["date","open","high","low","close","volume"]}"#,
        shape: ResponseShape::IntradayBar,
    },
    RestCase {
        tool: "get_boats_snapshot",
        arguments_json: r#"{"ticker":"AAPL"}"#,
        shape: ResponseShape::EquitySnapshot {
            ticker: "AAPL",
            price_fields: &[
                "last",
                "tngoLast",
                "mid",
                "bidPrice",
                "askPrice",
                "prevClose",
            ],
        },
    },
    RestCase {
        tool: "get_boats_prices",
        arguments_json: r#"{"ticker":"AAPL","resample_freq":"1hour","columns":["date","open","high","low","close","volume"]}"#,
        shape: ResponseShape::IntradayBar,
    },
    RestCase {
        tool: "get_fund_metadata",
        arguments_json: r#"{"ticker":"VFIAX"}"#,
        shape: ResponseShape::FundMetadata,
    },
    RestCase {
        tool: "get_fund_fee_metrics",
        arguments_json: r#"{"ticker":"VFIAX"}"#,
        shape: ResponseShape::FundMetrics,
    },
    RestCase {
        tool: "search_tiingo_assets",
        arguments_json: r#"{"query":"AAPL"}"#,
        shape: ResponseShape::SearchAsset,
    },
    RestCase {
        tool: "get_crypto_yield_platforms",
        arguments_json: r#"{"platform_codes":["AAVEV2"]}"#,
        shape: ResponseShape::RequiredRows {
            identity: Some(("platformCode", "AAVEV2")),
            strings: &["name", "network"],
            numbers: &[],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_crypto_yield_pools",
        arguments_json: r#"{"pool_codes":["aavev2_usdc"]}"#,
        shape: ResponseShape::RequiredRows {
            identity: Some(("poolCode", "aavev2_usdc")),
            strings: &["yieldPlatform"],
            numbers: &[],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_crypto_yield_ticks",
        arguments_json: r#"{"pool_codes":["aavev2_usdc"]}"#,
        shape: ResponseShape::RequiredRows {
            identity: Some(("poolCode", "aavev2_usdc")),
            strings: &["date"],
            numbers: &["supplyRate"],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_crypto_yield_metrics",
        arguments_json: r#"{"pool_code":"aavev2_usdc","start_date":"2024-01-01","end_date":"2024-01-02","resample_freq":"5min"}"#,
        shape: ResponseShape::YieldMetric,
    },
    RestCase {
        tool: "get_forex_quote",
        arguments_json: r#"{"ticker":"eurusd"}"#,
        shape: ResponseShape::ForexQuote { ticker: "eurusd" },
    },
    RestCase {
        tool: "get_forex_quotes",
        arguments_json: r#"{"tickers":["eurusd"]}"#,
        shape: ResponseShape::ForexQuote { ticker: "eurusd" },
    },
    RestCase {
        tool: "get_forex_prices",
        arguments_json: r#"{"ticker":"eurusd","start_date":"2024-01-02","end_date":"2024-01-02","resample_freq":"1hour"}"#,
        shape: ResponseShape::PriceBar,
    },
    RestCase {
        tool: "get_crypto_quote",
        arguments_json: r#"{"tickers":"btcusd"}"#,
        shape: ResponseShape::CryptoQuote { ticker: "btcusd" },
    },
    RestCase {
        tool: "get_crypto_prices",
        arguments_json: r#"{"tickers":"btcusd","start_date":"2024-01-02","end_date":"2024-01-02","resample_freq":"1hour"}"#,
        shape: ResponseShape::CryptoHistory,
    },
    RestCase {
        tool: "get_crypto_metadata",
        arguments_json: r#"{"tickers":"btcusd"}"#,
        shape: ResponseShape::RequiredRows {
            identity: Some(("ticker", "btcusd")),
            strings: &["baseCurrency", "quoteCurrency"],
            numbers: &[],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_news",
        arguments_json: r#"{"tickers":"AAPL","limit":1}"#,
        shape: ResponseShape::NewsArticle,
    },
    RestCase {
        tool: "get_fundamentals_definitions",
        arguments_json: r#"{}"#,
        shape: ResponseShape::FundamentalsDefinition,
    },
    RestCase {
        tool: "get_financial_statements",
        arguments_json: r#"{"ticker":"AAPL","start_date":"2024-01-01","end_date":"2024-03-31"}"#,
        shape: ResponseShape::RequiredRows {
            identity: None,
            strings: &["date"],
            numbers: &["year", "quarter"],
            objects: &["statementData"],
        },
    },
    RestCase {
        tool: "get_daily_fundamentals",
        arguments_json: r#"{"ticker":"AAPL","start_date":"2024-01-02","end_date":"2024-01-02","columns":["date","marketCap"]}"#,
        shape: ResponseShape::RequiredRows {
            identity: None,
            strings: &["date"],
            numbers: &["marketCap"],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_company_meta",
        arguments_json: r#"{"tickers":"AAPL","columns":["ticker","name"]}"#,
        shape: ResponseShape::RequiredRows {
            identity: Some(("ticker", "AAPL")),
            strings: &["name"],
            numbers: &[],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_distributions_by_ex_date",
        arguments_json: r#"{"ex_date":"2024-02-09"}"#,
        shape: ResponseShape::Dividend { ticker: "AAPL" },
    },
    RestCase {
        tool: "get_dividends",
        arguments_json: r#"{"ticker":"AAPL","start_date":"2024-02-09","end_date":"2024-02-09"}"#,
        shape: ResponseShape::Dividend { ticker: "AAPL" },
    },
    RestCase {
        tool: "get_dividend_yield",
        arguments_json: r#"{"ticker":"AAPL","start_date":"2024-01-02","end_date":"2024-01-02"}"#,
        shape: ResponseShape::RequiredRows {
            identity: None,
            strings: &["date"],
            numbers: &["trailingDiv1Y"],
            objects: &[],
        },
    },
    RestCase {
        tool: "get_splits",
        arguments_json: r#"{"ticker":"NVDA","start_date":"2024-06-10","end_date":"2024-06-10"}"#,
        shape: ResponseShape::Split { ticker: "NVDA" },
    },
    RestCase {
        tool: "get_splits_by_ex_date",
        arguments_json: r#"{"ex_date":"2024-06-10"}"#,
        shape: ResponseShape::Split { ticker: "NVDA" },
    },
];

#[allow(unused_macros)]
macro_rules! legacy_live_cases {
    ($emit:ident) => {
        $emit! {
            live_boats_single_ticker => live_boats_single_ticker_run : ["get_boats_snapshot", "get_boats_prices"],
            live_consolidated_equity_single_ticker => live_consolidated_equity_single_ticker_run : ["get_equity_realtime_snapshot", "get_equity_intraday_prices"],
            live_consolidated_level_six_single_ticker_websocket => live_consolidated_level_six_single_ticker_websocket_run : ["start_market_data_subscription", "poll_market_data_subscription", "stop_market_data_subscription"],
            live_crypto_yield_metrics_single_pool => live_crypto_yield_metrics_single_pool_run : ["get_crypto_yield_metrics"],
            live_distributions_by_ex_date_tiny_filter => live_distributions_by_ex_date_tiny_filter_run : ["get_distributions_by_ex_date"],
            live_forex_quotes_single_pair => live_forex_quotes_single_pair_run : ["get_forex_quotes"],
            live_fund_fees_single_ticker => live_fund_fees_single_ticker_run : ["get_fund_metadata", "get_fund_fee_metrics"],
            live_iex_level_six_single_ticker_websocket => live_iex_level_six_single_ticker_websocket_run : ["start_market_data_subscription", "poll_market_data_subscription", "stop_market_data_subscription"],
            live_read_only_tiingo_capabilities => live_read_only_tiingo_capabilities_run : ["get_stock_metadata", "get_stock_prices", "get_forex_quote", "get_crypto_quote", "get_news", "get_fundamentals_definitions", "get_dividends"],
            live_search_early_beta => live_search_early_beta_run : ["search_tiingo_assets"],
            live_splits_by_ex_date_tiny_filter => live_splits_by_ex_date_tiny_filter_run : ["get_splits_by_ex_date"],
        }
    };
}
#[allow(unused_imports)]
pub(crate) use legacy_live_cases;

macro_rules! declared_legacy_inventory {
    ($($name:ident => $runner:ident : [$($tool:literal),*]),* $(,)?) => {
        /// Return the executable legacy live-case names and their declared tool coverage.
        fn legacy_inventory() -> BTreeMap<String, BTreeSet<String>> {
            BTreeMap::from([$( (stringify!($name).to_owned(), BTreeSet::from([$($tool.to_owned()),*])) ),*])
        }
    };
}
legacy_live_cases!(declared_legacy_inventory);

/// Return legacy and actual-stdio live cases from one maintained inventory.
pub fn live_inventory() -> BTreeMap<String, BTreeSet<String>> {
    let mut inventory = legacy_inventory();
    inventory.insert(
        "live_mcp_eod_data_is_consistent_accurate_and_timely".to_owned(),
        BTreeSet::from(["get_stock_prices".to_owned()]),
    );
    inventory.insert(
        "live_mcp_bounded_rest_end_to_end".to_owned(),
        REST_CASES.iter().map(|case| case.tool.to_owned()).collect(),
    );
    for name in [
        "live_mcp_iex_subscription_update_end_to_end",
        "live_mcp_consolidated_subscription_update_end_to_end",
    ] {
        inventory.insert(
            name.to_owned(),
            BTreeSet::from([
                "start_market_data_subscription".to_owned(),
                "update_market_data_subscription".to_owned(),
                "poll_market_data_subscription".to_owned(),
                "stop_market_data_subscription".to_owned(),
            ]),
        );
    }
    inventory
}
