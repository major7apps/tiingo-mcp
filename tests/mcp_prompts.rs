use std::{collections::BTreeSet, time::Duration};

use rmcp::{
    RoleClient, ServiceExt,
    model::{GetPromptRequestParams, JsonObject},
    service::RunningService,
};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    mcp::TiingoServer,
};
use url::Url;

struct Connection {
    client: RunningService<RoleClient, ()>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Connection {
    async fn new() -> Self {
        let client = TiingoClient::new(Config {
            api_key: None,
            base_url: Url::parse("http://127.0.0.1:1").unwrap(),
            request_timeout: Duration::from_secs(1),
            retry: RetryPolicy::test(),
            max_response_bytes: 8 * 1024 * 1024,
        })
        .unwrap();
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let server = tokio::spawn(async move {
            TiingoServer::with_client(client)
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });
        Self {
            client: ().serve(client_transport).await.unwrap(),
            server,
        }
    }

    async fn close(mut self) {
        self.client.close().await.unwrap();
        self.server.await.unwrap().unwrap();
    }
}

struct ExpectedPrompt {
    name: &'static str,
    description: &'static str,
    required: &'static [&'static str],
    optional: &'static [&'static str],
}

const EXPECTED_PROMPTS: [ExpectedPrompt; 5] = [
    ExpectedPrompt {
        name: "analyze-stock",
        description: "Comprehensive single-stock analysis: metadata, prices, fundamentals, and news",
        required: &["ticker"],
        optional: &["include_news"],
    },
    ExpectedPrompt {
        name: "compare-stocks",
        description: "Side-by-side comparison of two stocks: prices, fundamentals, and performance",
        required: &["ticker1", "ticker2"],
        optional: &["period"],
    },
    ExpectedPrompt {
        name: "crypto-market-overview",
        description: "Crypto market snapshot: current prices, 24h changes, and 7-day trends",
        required: &[],
        optional: &["tickers"],
    },
    ExpectedPrompt {
        name: "earnings-report-analysis",
        description: "Analyze a stock's earnings report: financials, price reaction, and news sentiment",
        required: &["ticker", "earnings_date"],
        optional: &[],
    },
    ExpectedPrompt {
        name: "forex-pair-analysis",
        description: "Currency pair analysis: current rate, historical trend, and volatility",
        required: &["pair"],
        optional: &["period"],
    },
];

fn arguments(value: serde_json::Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

fn text(result: &rmcp::model::GetPromptResult) -> String {
    serde_json::to_value(&result.messages[0].content).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn advertises_legacy_prompts_with_compatible_arguments() {
    let connection = Connection::new().await;

    let prompts = connection.client.list_prompts(None).await.unwrap().prompts;
    assert_eq!(prompts.len(), EXPECTED_PROMPTS.len());
    assert_eq!(
        prompts
            .iter()
            .map(|prompt| prompt.name.as_ref())
            .collect::<BTreeSet<_>>(),
        EXPECTED_PROMPTS.iter().map(|prompt| prompt.name).collect(),
    );

    for expected in EXPECTED_PROMPTS {
        let prompt = prompts
            .iter()
            .find(|prompt| prompt.name == expected.name)
            .unwrap();
        assert_eq!(prompt.description.as_deref(), Some(expected.description));
        assert_eq!(
            prompt
                .arguments
                .as_ref()
                .unwrap()
                .iter()
                .filter(|argument| argument.required.unwrap_or(false))
                .map(|argument| argument.name.as_ref())
                .collect::<BTreeSet<_>>(),
            expected.required.iter().copied().collect(),
            "{} required arguments drifted",
            expected.name,
        );
        assert_eq!(
            prompt
                .arguments
                .as_ref()
                .unwrap()
                .iter()
                .filter(|argument| !argument.required.unwrap_or(false))
                .map(|argument| argument.name.as_ref())
                .collect::<BTreeSet<_>>(),
            expected.optional.iter().copied().collect(),
            "{} optional arguments drifted",
            expected.name,
        );
    }

    connection.close().await;
}

#[tokio::test]
async fn serves_defaults_and_corrected_prompt_guidance() {
    let connection = Connection::new().await;

    let analyze = connection
        .client
        .get_prompt(
            GetPromptRequestParams::new("analyze-stock")
                .with_arguments(arguments(serde_json::json!({"ticker": "AAPL"}))),
        )
        .await
        .unwrap();
    let analyze_text = text(&analyze);
    assert_eq!(
        analyze.description.as_deref(),
        Some(EXPECTED_PROMPTS[0].description)
    );
    assert!(analyze_text.contains("get_news with tickers=AAPL"));
    assert!(analyze_text.contains("identify reported catalysts and market-moving events"));
    assert!(analyze_text.contains(
        "**Recent Catalysts**: Reported news or events if news was fetched; do not infer causes absent evidence."
    ));
    assert!(!analyze_text.contains("identify key catalysts"));
    assert!(!analyze_text.contains("driving price movement"));
    assert!(
        analyze_text.find("get_company_meta").unwrap()
            < analyze_text.find("sector and industry").unwrap()
    );

    let without_news = connection
        .client
        .get_prompt(
            GetPromptRequestParams::new("analyze-stock").with_arguments(arguments(
                serde_json::json!({"ticker": "AAPL", "include_news": false}),
            )),
        )
        .await
        .unwrap();
    assert!(!text(&without_news).contains("get_news"));

    let compare = connection
        .client
        .get_prompt(
            GetPromptRequestParams::new("compare-stocks").with_arguments(arguments(
                serde_json::json!({"ticker1": "AAPL", "ticker2": "MSFT"}),
            )),
        )
        .await
        .unwrap();
    assert_eq!(
        compare.description.as_deref(),
        Some(EXPECTED_PROMPTS[1].description)
    );
    assert!(text(&compare).contains("past 3 months"));

    let crypto = connection
        .client
        .get_prompt(GetPromptRequestParams::new("crypto-market-overview"))
        .await
        .unwrap();
    assert_eq!(
        crypto.description.as_deref(),
        Some(EXPECTED_PROMPTS[2].description)
    );
    assert!(text(&crypto).contains("btcusd,ethusd,solusd"));

    let earnings = connection
        .client
        .get_prompt(
            GetPromptRequestParams::new("earnings-report-analysis").with_arguments(arguments(
                serde_json::json!({"ticker": "NVDA", "earnings_date": "2024-02-21"}),
            )),
        )
        .await
        .unwrap();
    assert_eq!(
        earnings.description.as_deref(),
        Some(EXPECTED_PROMPTS[3].description)
    );
    let earnings_text = text(&earnings).to_lowercase();
    assert!(!earnings_text.contains("based on trends"));
    assert!(earnings_text.contains("explicit consensus comparison"));

    let forex = connection
        .client
        .get_prompt(
            GetPromptRequestParams::new("forex-pair-analysis")
                .with_arguments(arguments(serde_json::json!({"pair": "eurusd"}))),
        )
        .await
        .unwrap();
    assert_eq!(
        forex.description.as_deref(),
        Some(EXPECTED_PROMPTS[4].description)
    );
    let forex_text = text(&forex);
    assert!(forex_text.contains("past 1 month"));
    assert!(forex_text.contains("price history alone cannot establish their cause"));
    assert!(!forex_text.contains("likely causes"));

    connection.close().await;
}

#[tokio::test]
async fn rejects_invalid_prompt_arguments_and_unknown_prompts() {
    let connection = Connection::new().await;

    assert!(
        connection
            .client
            .get_prompt(GetPromptRequestParams::new("analyze-stock"))
            .await
            .is_err()
    );
    assert!(
        connection
            .client
            .get_prompt(
                GetPromptRequestParams::new("analyze-stock").with_arguments(arguments(
                    serde_json::json!({"ticker": "AAPL", "include_news": "yes"})
                )),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .client
            .get_prompt(GetPromptRequestParams::new("unknown"))
            .await
            .is_err()
    );

    connection.close().await;
}
