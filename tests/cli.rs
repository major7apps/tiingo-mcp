use assert_cmd::Command;

#[test]
fn version_flag_reports_release_version() {
    Command::cargo_bin("tiingo-mcp")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout("tiingo-mcp 2.0.0\n");
}
