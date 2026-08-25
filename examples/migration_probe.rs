use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

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

#[derive(Debug)]
struct CleanupOutcome {
    forced: bool,
}

fn percentile_index(length: usize, percentile: f64) -> usize {
    ((percentile * length as f64).ceil() as usize).saturating_sub(1)
}

fn median_f64(sorted: &[f64]) -> f64 {
    assert!(!sorted.is_empty(), "median requires at least one sample");
    let upper = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[upper - 1] + sorted[upper]) / 2.0
    } else {
        sorted[upper]
    }
}

fn median_u64(sorted: &[u64]) -> u64 {
    assert!(!sorted.is_empty(), "median requires at least one sample");
    let upper = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        ((u128::from(sorted[upper - 1]) + u128::from(sorted[upper])) / 2) as u64
    } else {
        sorted[upper]
    }
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

fn extend_process_tree(system: &System, process_tree: &mut HashSet<Pid>) {
    loop {
        let descendants = system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                process
                    .parent()
                    .filter(|parent| process_tree.contains(parent))
                    .map(|_| *pid)
            })
            .filter(|pid| !process_tree.contains(pid))
            .collect::<Vec<_>>();
        if descendants.is_empty() {
            return;
        }
        process_tree.extend(descendants);
    }
}

fn refresh_process_tree(system: &mut System, root: Pid, process_tree: &mut HashSet<Pid>) {
    system.refresh_processes(ProcessesToUpdate::All, true);
    process_tree.insert(root);
    extend_process_tree(system, process_tree);
}

fn process_tree_rss_bytes(system: &mut System, child: &Child) -> anyhow::Result<u64> {
    let root = Pid::from(child.id() as usize);
    let mut process_tree = HashSet::from([root]);
    refresh_process_tree(system, root, &mut process_tree);
    ensure!(
        system.process(root).is_some(),
        "measured server process exited before RSS collection"
    );
    Ok(process_tree
        .iter()
        .filter_map(|pid| system.process(*pid))
        .map(|process| process.memory())
        .sum())
}

fn configure_containment(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
}

#[cfg(unix)]
fn containment_is_alive(root_pid: u32) -> anyhow::Result<bool> {
    let process_group = format!("-{root_pid}");
    Ok(Command::new("/bin/kill")
        .args(["-0", "--", &process_group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

#[cfg(windows)]
fn containment_is_alive(root_pid: u32) -> anyhow::Result<bool> {
    let mut system = System::new();
    let root = Pid::from(root_pid as usize);
    system.refresh_processes(ProcessesToUpdate::Some(&[root]), true);
    Ok(system.process(root).is_some())
}

#[cfg(unix)]
fn terminate_containment(child: &mut Child) -> anyhow::Result<()> {
    let root_pid = child.id();
    let process_group = format!("-{root_pid}");
    let status = Command::new("/bin/kill")
        .args(["-KILL", "--", &process_group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() && containment_is_alive(root_pid)? {
        anyhow::bail!("failed to terminate Unix process group {root_pid}");
    }
    if child.try_wait()?.is_none() {
        child.wait()?;
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_containment(child: &mut Child) -> anyhow::Result<()> {
    let root_pid = child.id().to_string();
    let status = Command::new("taskkill")
        .args(["/PID", &root_pid, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() && child.try_wait()?.is_none() {
        child.kill()?;
    }
    if child.try_wait()?.is_none() {
        child.wait()?;
    }
    Ok(())
}

fn force_cleanup_process_tree(child: &mut Child) -> anyhow::Result<CleanupOutcome> {
    let root_pid = child.id();
    terminate_containment(child)?;

    let confirmation_deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if !containment_is_alive(root_pid)? {
            return Ok(CleanupOutcome { forced: true });
        }
        ensure!(
            Instant::now() < confirmation_deadline,
            "failed to terminate complete launched containment"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn cleanup_process_tree(child: &mut Child, timeout: Duration) -> anyhow::Result<CleanupOutcome> {
    let root_pid = child.id();
    let deadline = Instant::now() + timeout;
    let mut root_status = None;

    loop {
        if root_status.is_none() {
            root_status = child.try_wait()?;
        }
        if !containment_is_alive(root_pid)?
            && let Some(status) = root_status.or(child.try_wait()?)
        {
            ensure!(status.success(), "server exited with {status}");
            return Ok(CleanupOutcome { forced: false });
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    force_cleanup_process_tree(child)
}

fn process_sample(command: &[String]) -> anyhow::Result<ProcessSample> {
    process_sample_with_timeout(command, EXIT_TIMEOUT)
}

fn process_sample_with_timeout(
    command: &[String],
    exit_timeout: Duration,
) -> anyhow::Result<ProcessSample> {
    let started = Instant::now();
    let mut child_command = Command::new(&command[0]);
    child_command
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_containment(&mut child_command);
    let mut child = child_command
        .spawn()
        .with_context(|| format!("failed to spawn {}", command[0]))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let (mut stdin, stdout) = match (stdin, stdout) {
        (Some(stdin), Some(stdout)) => (stdin, stdout),
        _ => {
            let handle_error = anyhow::anyhow!("child stdio was not piped");
            if let Err(cleanup_error) = force_cleanup_process_tree(&mut child) {
                eprintln!("cleanup after stdio error: {cleanup_error:#}");
            }
            return Err(handle_error);
        }
    };
    let mut stdout = BufReader::new(stdout);

    let probe_result = (|| {
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
        let rss_initialized_bytes = process_tree_rss_bytes(&mut system, &child)?;
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

        let rss_workload_bytes = process_tree_rss_bytes(&mut system, &child)?;
        Ok(ProcessSample {
            startup_ms,
            rss_initialized_bytes,
            rss_workload_bytes,
        })
    })();
    drop(stdin);
    let cleanup_result = if probe_result.is_err() {
        force_cleanup_process_tree(&mut child)
    } else {
        cleanup_process_tree(&mut child, exit_timeout)
    };
    match probe_result {
        Err(probe_error) => {
            if let Err(cleanup_error) = cleanup_result {
                eprintln!("cleanup after probe error: {cleanup_error:#}");
            }
            Err(probe_error)
        }
        Ok(sample) => {
            let cleanup = cleanup_result?;
            ensure!(
                !cleanup.forced,
                "server process tree did not exit within {:.3} seconds after stdin closed; terminated",
                exit_timeout.as_secs_f64()
            );
            Ok(sample)
        }
    }
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

    let p95 = percentile_index(runs, 0.95);
    println!(
        "{}",
        json!({
            "runs": runs,
            "startup_ms_median": median_f64(&startup_ms),
            "startup_ms_p95": startup_ms[p95],
            "rss_initialized_bytes_median": median_u64(&rss_initialized),
            "rss_workload_bytes_median": median_u64(&rss_workload),
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
            "wrapper_us_median": median_f64(&samples),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn medians_average_the_two_middle_values_for_even_samples() {
        assert_eq!(median_f64(&[1.0, 3.0]), 2.0);
        assert_eq!(median_u64(&[1, 3]), 2);
    }

    #[test]
    #[should_panic(expected = "median requires at least one sample")]
    fn floating_point_median_rejects_an_empty_sample() {
        median_f64(&[]);
    }

    #[test]
    #[should_panic(expected = "median requires at least one sample")]
    fn integer_median_rejects_an_empty_sample() {
        median_u64(&[]);
    }

    fn pid_file(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "migration-probe-{name}-{}-{nonce}.pid",
            std::process::id()
        ))
    }

    fn read_descendant_pid(path: &Path) -> Pid {
        let pid = fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap();
        fs::remove_file(path).unwrap();
        Pid::from(pid)
    }

    fn assert_process_is_gone(pid: Pid) {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut system = System::new();
        loop {
            system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
            if system.process(pid).is_none() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "long-lived descendant {pid} survived cleanup"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn early_error_command() -> (Vec<String>, PathBuf) {
        let pid_file = pid_file("early-error");
        let command = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!(
                "sleep 60 & descendant=$!; printf '%s\\n' \"$descendant\" > '{}'; printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{{\"code\":-32602,\"message\":\"early protocol error\"}}}}'; wait",
                pid_file.display()
            ),
        ];
        (command, pid_file)
    }

    #[cfg(windows)]
    fn early_error_command() -> (Vec<String>, PathBuf) {
        let pid_file = pid_file("early-error");
        let script = format!(
            "$child=Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 60' -PassThru; Set-Content -NoNewline -Path '{}' -Value $child.Id; Write-Output '{{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{{\"code\":-32602,\"message\":\"early protocol error\"}}}}'; Wait-Process -Id $child.Id",
            pid_file.display()
        );
        (
            vec![
                "powershell".to_owned(),
                "-NoProfile".to_owned(),
                "-Command".to_owned(),
                script,
            ],
            pid_file,
        )
    }

    #[cfg(unix)]
    fn ignores_eof_command() -> (Vec<String>, PathBuf) {
        let pid_file = pid_file("ignores-eof");
        let command = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!(
                "sleep 60 & descendant=$!; printf '%s\\n' \"$descendant\" > '{}'; i=1; while [ $i -le 11 ]; do printf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{}}}}\\n' \"$i\"; i=$((i + 1)); done; wait",
                pid_file.display()
            ),
        ];
        (command, pid_file)
    }

    #[cfg(unix)]
    fn exits_on_eof_command() -> Vec<String> {
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "i=1; while [ $i -le 11 ]; do printf '{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{}}\\n' \"$i\"; i=$((i + 1)); done; cat >/dev/null"
                .to_owned(),
        ]
    }

    #[cfg(unix)]
    fn descendant_command() -> (Command, PathBuf) {
        let pid_file = pid_file("rss");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!(
                "sleep 60 & descendant=$!; printf '%s\\n' \"$descendant\" > '{}'; wait",
                pid_file.display()
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_containment(&mut command);
        (command, pid_file)
    }

    #[cfg(windows)]
    fn descendant_command() -> (Command, PathBuf) {
        let pid_file = pid_file("rss");
        let mut command = Command::new("powershell");
        command
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "$child=Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 60' -PassThru; Set-Content -NoNewline -Path '{}' -Value $child.Id; Wait-Process -Id $child.Id",
                    pid_file.display()
                ),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_containment(&mut command);
        (command, pid_file)
    }

    #[cfg(windows)]
    fn ignores_eof_command() -> (Vec<String>, PathBuf) {
        let pid_file = pid_file("ignores-eof");
        let responses = (1..=11)
            .map(|id| format!("Write-Output '{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}'"))
            .collect::<Vec<_>>()
            .join("; ");
        let script = format!(
            "$child=Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 60' -PassThru; Set-Content -NoNewline -Path '{}' -Value $child.Id; {responses}; Wait-Process -Id $child.Id",
            pid_file.display()
        );
        (
            vec![
                "powershell".to_owned(),
                "-NoProfile".to_owned(),
                "-Command".to_owned(),
                script,
            ],
            pid_file,
        )
    }

    #[cfg(windows)]
    fn exits_on_eof_command() -> Vec<String> {
        let responses = (1..=11)
            .map(|id| format!("Write-Output '{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}'"))
            .collect::<Vec<_>>()
            .join("; ");
        vec![
            "powershell".to_owned(),
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!("{responses}; $input | Out-Null"),
        ]
    }

    #[test]
    fn early_protocol_error_is_preserved_after_bounded_cleanup() {
        let (command, pid_file) = early_error_command();
        let started = Instant::now();
        let error = process_sample_with_timeout(&command, Duration::from_millis(200)).unwrap_err();
        let descendant = read_descendant_pid(&pid_file);

        assert!(
            error.to_string().contains("request 1 failed"),
            "original protocol error was replaced: {error:#}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_process_is_gone(descendant);
    }

    #[test]
    fn child_that_ignores_eof_is_terminated_within_the_bound() {
        let (command, pid_file) = ignores_eof_command();
        let started = Instant::now();
        let error = process_sample_with_timeout(&command, Duration::from_millis(200)).unwrap_err();
        let descendant = read_descendant_pid(&pid_file);

        assert!(
            error.to_string().contains("did not exit within"),
            "unexpected cleanup error: {error:#}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_process_is_gone(descendant);
    }

    #[test]
    fn child_that_exits_on_eof_is_reaped_without_a_cleanup_error() {
        process_sample_with_timeout(&exits_on_eof_command(), Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn rss_includes_resident_descendants_in_the_launched_process_tree() {
        let (mut command, pid_file) = descendant_command();
        let mut child = command.spawn().unwrap();
        let root = Pid::from(child.id() as usize);
        let mut system = System::new();
        let deadline = Instant::now() + Duration::from_secs(1);
        let root_rss = loop {
            let mut process_tree = HashSet::from([root]);
            refresh_process_tree(&mut system, root, &mut process_tree);
            if process_tree.len() > 1 {
                break system.process(root).unwrap().memory();
            }
            assert!(Instant::now() < deadline, "descendant did not start");
            thread::sleep(Duration::from_millis(10));
        };

        let aggregate_rss = process_tree_rss_bytes(&mut system, &child).unwrap();
        drop(child.stdin.take());
        let cleanup = cleanup_process_tree(&mut child, Duration::from_millis(200)).unwrap();
        let descendant = read_descendant_pid(&pid_file);

        assert!(
            aggregate_rss > root_rss,
            "aggregate RSS {aggregate_rss} did not exceed root-only RSS {root_rss}"
        );
        assert!(
            cleanup.forced,
            "stubborn descendant exited without containment cleanup"
        );
        assert_process_is_gone(descendant);
    }
}
