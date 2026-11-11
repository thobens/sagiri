use assert_cmd::Command;

#[test]
fn version_flag_prints_version() {
    Command::cargo_bin("sagiri")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout("sagiri 0.1.0\n");
}
