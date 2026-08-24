use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, ensure};
use clap::Parser;
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, JsonObject},
    service::RunningService,
};
use serde_json::{Value, json};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, MAX_RESPONSE_BYTES, RetryPolicy},
    mcp::TiingoServer,
};
use url::Url;
use wait_timeout::ChildExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const WRAPPER_WARMUP_RUNS: usize = 10;
const REQUIRED_WRAPPER_RUNS: usize = 1_000;
const FIXED_RESOURCES: [&str; 3] = [
    "tiingo://capabilities",
    "tiingo://fundamentals/definitions",
    "tiingo://guide/date-formats",
];

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, value_name = "N", conflicts_with = "wrapper_runs")]
    runs: Option<usize>,

    #[arg(long, value_name = "N", conflicts_with = "runs")]
    wrapper_runs: Option<usize>,

    #[arg(last = true)]
    command: Vec<String>,
}

#[derive(Debug)]
struct ProcessSample {
    startup_ms: f64,
    rss_initialized_bytes: u64,
    rss_workload_bytes: u64,
}

fn percentile_index(length: usize, percentile: f64) -> usize {
    ((percentile * length as f64).ceil() as usize).saturating_sub(1)
}

fn write_message(stdin: &mut impl Write, message: &Value) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *stdin, message)?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn read_successful_response(stdout: &mut impl BufRead, expected_id: u64) -> anyhow::Result<Value> {
    loop {
        let mut line = String::new();
        ensure!(stdout.read_line(&mut line)? != 0, "server closed stdout");
        let response: Value = serde_json::from_str(&line)
            .with_context(|| format!("server emitted invalid JSON: {line:?}"))?;
        if response.get("id") != Some(&json!(expected_id)) {
            continue;
        }
        ensure!(
            response.get("error").is_none(),
            "request {expected_id} failed: {response}"
        );
        ensure!(
            response.get("result").is_some(),
            "request {expected_id} omitted result: {response}"
        );
        return Ok(response);
    }
}

fn request(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: u64,
    method: &str,
    params: Value,
) -> anyhow::Result<()> {
    write_message(
        stdin,
        &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )?;
    read_successful_response(stdout, id)?;
    Ok(())
}

fn rss_bytes(system: &mut System, child: &Child) -> anyhow::Result<u64> {
    let pid = Pid::from(child.id() as usize);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .map(|process| process.memory())
        .context("measured server process exited before RSS collection")
}

fn wait_for_exit(child: &mut Child) -> anyhow::Result<()> {
    if let Some(status) = child.wait_timeout(EXIT_TIMEOUT)? {
        ensure!(status.success(), "server exited with {status}");
        return Ok(());
    }
    child.kill()?;
    child.wait()?;
    anyhow::bail!("server did not exit within five seconds after stdin closed")
}

fn process_sample(command: &[String]) -> anyhow::Result<ProcessSample> {
    let started = Instant::now();
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn {}", command[0]))?;
    let mut stdin = child.stdin.take().context("child stdin was not piped")?;
    let stdout = child.stdout.take().context("child stdout was not piped")?;
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "migration-probe", "version": "1.0.0"}
            }
        }),
    )?;
    read_successful_response(&mut stdout, 1)?;
    let startup_ms = started.elapsed().as_secs_f64() * 1_000.0;

    let mut system = System::new();
    let rss_initialized_bytes = rss_bytes(&mut system, &child)?;
    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
    )?;

    request(&mut stdin, &mut stdout, 2, "tools/list", json!({}))?;
    request(&mut stdin, &mut stdout, 3, "resources/list", json!({}))?;

    let mut id = 4;
    for uri in FIXED_RESOURCES {
        request(
            &mut stdin,
            &mut stdout,
            id,
            "resources/read",
            json!({"uri": uri}),
        )?;
        id += 1;
    }

    let prompt_cases = [
        json!({"name": "analyze-stock", "arguments": {"ticker": "AAPL"}}),
        json!({
            "name": "compare-stocks",
            "arguments": {"ticker1": "AAPL", "ticker2": "MSFT"}
        }),
        json!({"name": "crypto-market-overview", "arguments": {}}),
        json!({
            "name": "earnings-report-analysis",
            "arguments": {"ticker": "NVDA", "earnings_date": "2024-02-21"}
        }),
        json!({"name": "forex-pair-analysis", "arguments": {"pair": "eurusd"}}),
    ];
    for params in prompt_cases {
        request(&mut stdin, &mut stdout, id, "prompts/get", params)?;
        id += 1;
    }

    let rss_workload_bytes = rss_bytes(&mut system, &child)?;
    drop(stdin);
    wait_for_exit(&mut child)?;

    Ok(ProcessSample {
        startup_ms,
        rss_initialized_bytes,
        rss_workload_bytes,
    })
}

fn process_probe(runs: usize, command: &[String]) -> anyhow::Result<()> {
    ensure!(runs > 0, "--runs must be greater than zero");
    ensure!(!command.is_empty(), "--runs requires a command after --");

    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        samples.push(process_sample(command)?);
    }

    let mut startup_ms = samples
        .iter()
        .map(|sample| sample.startup_ms)
        .collect::<Vec<_>>();
    let mut rss_initialized = samples
        .iter()
        .map(|sample| sample.rss_initialized_bytes)
        .collect::<Vec<_>>();
    let mut rss_workload = samples
        .iter()
        .map(|sample| sample.rss_workload_bytes)
        .collect::<Vec<_>>();
    startup_ms.sort_by(f64::total_cmp);
    rss_initialized.sort_unstable();
    rss_workload.sort_unstable();

    let median = (runs - 1) / 2;
    let p95 = percentile_index(runs, 0.95);
    println!(
        "{}",
        json!({
            "runs": runs,
            "startup_ms_median": startup_ms[median],
            "startup_ms_p95": startup_ms[p95],
            "rss_initialized_bytes_median": rss_initialized[median],
            "rss_workload_bytes_median": rss_workload[median],
        })
    );
    Ok(())
}

struct Connection {
    client: RunningService<RoleClient, ()>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Connection {
    async fn new(client: TiingoClient) -> anyhow::Result<Self> {
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let server = tokio::spawn(async move {
            TiingoServer::with_client(client)
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;
        Ok(Self { client, server })
    }

    async fn close(mut self) -> anyhow::Result<()> {
        self.client.close().await?;
        self.server.await??;
        Ok(())
    }
}

fn arguments(value: Value) -> JsonObject {
    value.as_object().expect("arguments are an object").clone()
}

async fn call_stock_metadata(client: &RunningService<RoleClient, ()>) -> anyhow::Result<()> {
    let result = client
        .call_tool(
            CallToolRequestParams::new("get_stock_metadata")
                .with_arguments(arguments(json!({"ticker": "AAPL"}))),
        )
        .await?;
    ensure!(
        result.is_error != Some(true),
        "wrapper call returned an error"
    );
    Ok(())
}

async fn wrapper_probe(runs: usize) -> anyhow::Result<()> {
    ensure!(
        runs == REQUIRED_WRAPPER_RUNS,
        "--wrapper-runs must be exactly {REQUIRED_WRAPPER_RUNS}"
    );

    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tiingo/daily/AAPL"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ticker": "AAPL"})))
        .mount(&upstream)
        .await;
    let fixture_url = Url::parse(&upstream.uri())?;
    let client = TiingoClient::new(Config {
        api_key: Some("benchmark-key".to_owned()),
        base_url: fixture_url,
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: MAX_RESPONSE_BYTES,
    })?;
    let connection = Connection::new(client).await?;

    for _ in 0..WRAPPER_WARMUP_RUNS {
        call_stock_metadata(&connection.client).await?;
    }
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        call_stock_metadata(&connection.client).await?;
        samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }

    let requests = upstream
        .received_requests()
        .await
        .context("local fixture did not retain received requests")?;
    ensure!(
        requests.len() == WRAPPER_WARMUP_RUNS + runs,
        "local fixture received {} requests, expected {}",
        requests.len(),
        WRAPPER_WARMUP_RUNS + runs
    );
    connection.close().await?;

    samples.sort_by(f64::total_cmp);
    println!(
        "{}",
        json!({
            "runs": runs,
            "wrapper_us_median": samples[(runs - 1) / 2],
            "wrapper_us_p95": samples[percentile_index(runs, 0.95)],
        })
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match (args.runs, args.wrapper_runs) {
        (Some(runs), None) => process_probe(runs, &args.command),
        (None, Some(runs)) => {
            ensure!(
                args.command.is_empty(),
                "--wrapper-runs does not accept a command"
            );
            wrapper_probe(runs).await
        }
        (None, None) => anyhow::bail!("provide --runs N -- <command> or --wrapper-runs 1000"),
        (Some(_), Some(_)) => unreachable!("clap enforces conflicts"),
    }
}
