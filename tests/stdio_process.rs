use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use anyhow::Context;
use assert_cmd::Command as AssertCommand;
use predicates::prelude::*;
use rmcp::{ClientHandler, ServiceExt};
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

async fn initialize_discover_and_cancel() -> anyhow::Result<()> {
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
    Ok(())
}

#[tokio::test]
async fn stdio_supports_python_compatible_initialize_and_current_discover() -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), initialize_discover_and_cancel())
        .await
        .context("stdio initialize/discover/cancel timed out")??;
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
    assert!(
        stderr.contains("rmcp::service") && stderr.contains("Service initialized"),
        "missing RMCP diagnostic: {stderr}"
    );
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
