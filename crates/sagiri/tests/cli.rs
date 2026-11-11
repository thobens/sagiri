use assert_cmd::Command;
use predicates::str::contains;

const ABOUT: &str = "Obfuscates pasted text with consistent, realistic-looking fake values";

fn sagiri() -> Command {
    Command::cargo_bin("sagiri").unwrap()
}

#[test]
fn version_flag_prints_version() {
    sagiri()
        .arg("--version")
        .assert()
        .success()
        .stdout("sagiri 0.1.0\n");
}

#[test]
fn help_flag_prints_about() {
    sagiri()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains(ABOUT));
}

#[test]
fn no_arguments_prints_help_and_succeeds() {
    sagiri()
        .assert()
        .success()
        .stdout(contains("Usage: sagiri"))
        .stdout(contains(ABOUT));
}
