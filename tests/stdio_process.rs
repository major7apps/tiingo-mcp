use std::{
    io::{BufRead, BufReader, Read, Write},
    pin::Pin,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    task::{Context as TaskContext, Poll},
    time::{Duration, Instant},
};

use anyhow::Context;
use assert_cmd::Command as AssertCommand;
use futures_util::{SinkExt, StreamExt};
use predicates::prelude::*;
use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParams, JsonObject},
};
use tiingo_mcp::{
    client::TiingoClient,
    config::{Config, RetryPolicy},
    mcp::TiingoServer,
    websocket::{
        protocol::Service,
        registry::{MarketDataRegistry, StartRequest, TiingoConnector},
    },
};
use tokio::io::AsyncWrite;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use url::Url;
use wait_timeout::ChildExt;

#[derive(Debug, Clone)]
struct VersionedClient(rmcp::model::ProtocolVersion);

impl ClientHandler for VersionedClient {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        let mut info = rmcp::model::ClientInfo::default();
        info.protocol_version = self.0.clone();
        info
    }
}

struct RecordingWriter<W> {
    inner: W,
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl<W> RecordingWriter<W> {
    fn new(inner: W) -> (Self, Arc<Mutex<Vec<u8>>>) {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                inner,
                bytes: Arc::clone(&bytes),
            },
            bytes,
        )
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for RecordingWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(context, buffer) {
            Poll::Ready(Ok(written)) => {
                this.bytes
                    .lock()
                    .unwrap()
                    .extend_from_slice(&buffer[..written]);
                Poll::Ready(Ok(written))
            }
            other => other,
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}

fn assert_protocol_only(bytes: &Arc<Mutex<Vec<u8>>>) {
    let bytes = bytes.lock().unwrap();
    let output = std::str::from_utf8(&bytes).expect("stdio output is UTF-8 JSON Lines");
    for line in output.lines().filter(|line| !line.is_empty()) {
        let message: serde_json::Value =
            serde_json::from_str(line).expect("stdio output contains no diagnostic text");
        assert_eq!(message["jsonrpc"], "2.0");
    }
}

fn child_process() -> std::io::Result<Child> {
    Command::new(env!("CARGO_BIN_EXE_tiingo-mcp"))
        .env_remove("TIINGO_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn wait_for_exit(child: &mut Child) -> std::io::Result<std::process::ExitStatus> {
    if let Some(status) = child.wait_timeout(Duration::from_secs(5))? {
        return Ok(status);
    }
    child.kill()?;
    child.wait()?;
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "tiingo-mcp did not exit within five seconds",
    ))
}

fn arguments(value: serde_json::Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

fn test_client(api_key: &str) -> TiingoClient {
    TiingoClient::new(Config {
        api_key: Some(api_key.to_owned()),
        base_url: Url::parse("http://127.0.0.1:1").unwrap(),
        request_timeout: Duration::from_secs(1),
        retry: RetryPolicy::test(),
        max_response_bytes: 8 * 1024 * 1024,
    })
    .unwrap()
}

async fn local_registry(
    api_key: &str,
) -> (
    MarketDataRegistry,
    tokio::task::JoinHandle<serde_json::Value>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let websocket = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::text(
                serde_json::json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 71},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .unwrap();
        let unsubscribe = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let unsubscribe = serde_json::from_str(&unsubscribe).unwrap();
        let _ = socket.next().await;
        unsubscribe
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint).unwrap();
    (
        MarketDataRegistry::with_connector(Some(api_key.into()), connector),
        websocket,
    )
}

async fn initialize_discover_and_cancel() -> anyhow::Result<Duration> {
    let started = Instant::now();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_tiingo-mcp"));
    command.env_remove("TIINGO_API_KEY");
    let transport = rmcp::transport::TokioChildProcess::new(command)?;
    let client = VersionedClient(rmcp::model::ProtocolVersion::V_2025_11_25)
        .serve(transport)
        .await?;
    assert_eq!(
        client
            .peer_info()
            .expect("initialize peer info")
            .protocol_version,
        rmcp::model::ProtocolVersion::V_2025_11_25,
    );

    let mut meta = rmcp::model::RequestMetaObject::new();
    meta.set_protocol_version(rmcp::model::ProtocolVersion::V_2026_07_28);
    meta.set_client_info(rmcp::model::Implementation::new("contract-test", "2.0.0"));
    meta.set_client_capabilities(rmcp::model::ClientCapabilities::default());
    let discovery = client.discover(meta).await?;
    assert_eq!(
        discovery.supported_versions,
        vec![
            rmcp::model::ProtocolVersion::V_2024_11_05,
            rmcp::model::ProtocolVersion::V_2025_03_26,
            rmcp::model::ProtocolVersion::V_2025_06_18,
            rmcp::model::ProtocolVersion::V_2025_11_25,
            rmcp::model::ProtocolVersion::V_2026_07_28,
        ]
    );
    assert!(discovery.capabilities.tools.is_some());
    assert!(discovery.capabilities.resources.is_some());
    assert!(discovery.capabilities.prompts.is_some());
    client.cancel().await?;
    Ok(started.elapsed())
}

#[tokio::test]
async fn stdio_supports_v1_compatible_initialize_and_current_discover() -> anyhow::Result<()> {
    let elapsed = tokio::time::timeout(Duration::from_secs(5), initialize_discover_and_cancel())
        .await
        .context("stdio initialize/discover/cancel timed out")??;
    eprintln!("stdio initialize/discover/cancel latency: {elapsed:?}");
    Ok(())
}

#[tokio::test]
async fn stdio_eof_shuts_down_an_active_market_data_worker() -> anyhow::Result<()> {
    let (registry, websocket) = local_registry("stdio-ws-secret").await;
    let server =
        TiingoServer::with_client_and_registry(test_client("stdio-ws-secret"), registry.clone());
    let (server_io, client_io) = tokio::io::duplex(16 * 1024);
    let (server_input, server_output) = tokio::io::split(server_io);
    let (server_output, stdout) = RecordingWriter::new(server_output);
    let owner = tokio::spawn(tiingo_mcp::run_stdio_with(
        server,
        server_input,
        server_output,
    ));
    let client = ().serve(client_io).await?;
    let started = client
        .call_tool(
            CallToolRequestParams::new("start_market_data_subscription").with_arguments(arguments(
                serde_json::json!({"service": "iex", "symbols": ["AAPL"]}),
            )),
        )
        .await?;
    anyhow::ensure!(started.is_error == Some(false));

    client.cancel().await?;
    tokio::time::timeout(Duration::from_millis(500), owner)
        .await
        .context("stdio EOF did not await registry shutdown")???;
    let unsubscribe = tokio::time::timeout(Duration::from_millis(500), websocket)
        .await
        .context("stdio EOF leaked the WebSocket worker")??;
    assert_eq!(
        unsubscribe,
        serde_json::json!({
            "eventName": "unsubscribe",
            "authorization": "stdio-ws-secret",
            "eventData": {"subscriptionId": 71, "tickers": ["AAPL"]}
        })
    );
    assert_protocol_only(&stdout);
    Ok(())
}

#[tokio::test]
async fn stdio_initialization_connection_loss_shuts_down_existing_workers() -> anyhow::Result<()> {
    let (registry, websocket) = local_registry("stdio-init-secret").await;
    registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await?;
    let server =
        TiingoServer::with_client_and_registry(test_client("stdio-init-secret"), registry.clone());
    let (server_io, client_io) = tokio::io::duplex(16 * 1024);
    let (server_input, server_output) = tokio::io::split(server_io);
    let (server_output, stdout) = RecordingWriter::new(server_output);
    drop(client_io);

    tokio::time::timeout(
        Duration::from_millis(500),
        tiingo_mcp::run_stdio_with(server, server_input, server_output),
    )
    .await
    .context("initialization connection loss did not return promptly")??;
    let unsubscribe = tokio::time::timeout(Duration::from_millis(500), websocket)
        .await
        .context("initialization connection loss leaked the WebSocket worker")??;
    assert_eq!(
        unsubscribe,
        serde_json::json!({
            "eventName": "unsubscribe",
            "authorization": "stdio-init-secret",
            "eventData": {"subscriptionId": 71, "tickers": ["AAPL"]}
        })
    );
    assert_protocol_only(&stdout);
    Ok(())
}

#[test]
fn closing_stdin_terminates_the_server_within_five_seconds() -> anyhow::Result<()> {
    let mut child = child_process()?;
    drop(child.stdin.take());

    let status = wait_for_exit(&mut child)?;
    assert!(status.success(), "server exited with {status}");
    Ok(())
}

#[test]
fn debug_diagnostics_stay_on_stderr_and_stdout_is_protocol_only() -> anyhow::Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tiingo-mcp"))
        .env_remove("TIINGO_API_KEY")
        .env("RUST_LOG", "debug")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (first_line_tx, first_line_rx) = mpsc::channel();
    let stdout_reader = std::thread::spawn(move || -> std::io::Result<String> {
        let mut reader = BufReader::new(stdout);
        let mut output = String::new();
        reader.read_line(&mut output)?;
        first_line_tx.send(output.clone()).unwrap();
        reader.read_to_string(&mut output)?;
        Ok(output)
    });
    let stderr_reader = std::thread::spawn(move || -> std::io::Result<String> {
        let mut output = String::new();
        BufReader::new(stderr).read_to_string(&mut output)?;
        Ok(output)
    });

    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin.write_all(
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"stdout-test\",\"version\":\"1.0.0\"}}}\n",
    )?;
    stdin.flush()?;

    let first_line = first_line_rx.recv_timeout(Duration::from_secs(5))?;
    let initialize: serde_json::Value = serde_json::from_str(&first_line)?;
    assert_eq!(initialize["jsonrpc"], "2.0");
    assert_eq!(initialize["id"], 1);
    assert!(initialize.get("result").is_some());

    drop(stdin);
    let status = wait_for_exit(&mut child)?;
    assert!(status.success(), "server exited with {status}");

    let stdout = stdout_reader.join().expect("stdout reader")?;
    for line in stdout.lines() {
        let message: serde_json::Value = serde_json::from_str(line)?;
        assert_eq!(message["jsonrpc"], "2.0");
    }
    let stderr = stderr_reader.join().expect("stderr reader")?;
    assert!(!stderr.is_empty(), "RUST_LOG=debug produced no diagnostics");
    assert!(!stdout.contains("DEBUG"));
    assert!(!stdout.contains("tiingo_mcp"));
    Ok(())
}

#[test]
fn help_exits_without_starting_mcp() {
    AssertCommand::cargo_bin("tiingo-mcp")
        .unwrap()
        .arg("--help")
        .timeout(Duration::from_secs(5))
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: tiingo-mcp"))
        .stdout(predicate::str::contains("--version"))
        .stderr("");
}

#[test]
fn version_exits_without_starting_mcp() {
    AssertCommand::cargo_bin("tiingo-mcp")
        .unwrap()
        .arg("--version")
        .timeout(Duration::from_secs(5))
        .assert()
        .success()
        .stdout(format!("tiingo-mcp {}\n", env!("CARGO_PKG_VERSION")))
        .stderr("");
}
