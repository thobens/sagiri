use assert_cmd::Command;
use predicates::str::contains;

const ABOUT: &str = "Sagiri obfuscates pasted text by replacing sensitive values with consistent, realistic-looking fake ones.";

fn sagiri() -> Command {
    Command::cargo_bin("sagiri").unwrap()
}

#[test]
fn version_flag_prints_version() {
    sagiri().arg("--version").assert().success().stdout(concat!(
        "sagiri ",
        env!("CARGO_PKG_VERSION"),
        "\n"
    ));
}

#[test]
fn help_flag_prints_about() {
    sagiri()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains(ABOUT));
}
