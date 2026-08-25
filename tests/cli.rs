use assert_cmd::Command;

#[test]
fn version_flag_reports_release_version() {
    Command::cargo_bin("tiingo-mcp")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("tiingo-mcp {}\n", env!("CARGO_PKG_VERSION")));
}
