use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

/// A command that sees no real key or config: everything points into `home`.
fn sagiri(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("sagiri").unwrap();
    cmd.env_remove("SAGIRI_CONFIG")
        .env("SAGIRI_KEY_FILE", home.join("key"))
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"));
    cmd
}

fn out(cmd: &mut Command, input: &str) -> String {
    let o = cmd
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .clone();
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn config_flag_is_used() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(&cfg, "allow = [\"198.51.100.7\"]\n").unwrap();
    let kept = out(
        sagiri(dir.path()).arg("--config").arg(&cfg),
        "ip 198.51.100.7\n",
    );
    assert_eq!(kept, "ip 198.51.100.7\n");
    let replaced = out(&mut sagiri(dir.path()), "ip 198.51.100.7\n");
    assert_ne!(replaced, "ip 198.51.100.7\n");
}

#[test]
fn env_config_is_used_and_flag_wins() {
    let dir = TempDir::new().unwrap();
    let env_cfg = dir.path().join("env.toml");
    let flag_cfg = dir.path().join("flag.toml");
    fs::write(&env_cfg, "allow = [\"198.51.100.7\"]\n").unwrap();
    fs::write(&flag_cfg, "").unwrap();
    let input = "ip 198.51.100.7\n";
    assert_eq!(
        out(sagiri(dir.path()).env("SAGIRI_CONFIG", &env_cfg), input),
        input
    );
    let o = out(
        sagiri(dir.path())
            .env("SAGIRI_CONFIG", &env_cfg)
            .arg("--config")
            .arg(&flag_cfg),
        input,
    );
    assert_ne!(o, input);
}

#[test]
fn terms_are_replaced() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(&cfg, "terms = [\"Project Falcon\"]\n").unwrap();
    let o = out(
        sagiri(dir.path()).arg("--config").arg(&cfg),
        "about Project Falcon\n",
    );
    assert!(o.starts_with("about ") && !o.contains("Falcon"), "{o}");
}

#[test]
fn missing_flag_file_exits_1_and_names_it() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope.toml");
    let o = sagiri(dir.path())
        .arg("--config")
        .arg(&missing)
        .write_stdin("a 198.51.100.7\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(o.stdout.is_empty());
    let stderr = String::from_utf8(o.stderr).unwrap();
    assert!(stderr.starts_with("sagiri: "), "{stderr}");
    assert!(stderr.contains(missing.to_str().unwrap()), "{stderr}");
}

#[test]
fn xdg_config_home_is_used() {
    let dir = TempDir::new().unwrap();
    let xdg = dir.path().join("xdg-config");
    fs::create_dir_all(xdg.join("sagiri")).unwrap();
    fs::write(
        xdg.join("sagiri/config.toml"),
        "allow = [\"198.51.100.7\"]\n",
    )
    .unwrap();
    let input = "ip 198.51.100.7\n";
    assert_eq!(
        out(sagiri(dir.path()).env("XDG_CONFIG_HOME", &xdg), input),
        input
    );
}

#[test]
fn unknown_key_exits_1_with_empty_stdout() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(&cfg, "allows = [\"hushed\"]\n").unwrap();
    let o = sagiri(dir.path())
        .arg("--config")
        .arg(&cfg)
        .write_stdin("a 198.51.100.7\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(o.stdout.is_empty());
    let stderr = String::from_utf8(o.stderr).unwrap();
    assert!(
        stderr.contains("allows") && stderr.contains("line 1"),
        "{stderr}"
    );
    assert!(!stderr.contains("hushed"), "{stderr}");
}

#[test]
fn overlong_term_exits_1() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(&cfg, format!("terms = [\"{}\"]\n", "a".repeat(200_000))).unwrap();
    let o = sagiri(dir.path())
        .arg("--config")
        .arg(&cfg)
        .write_stdin("a 198.51.100.7\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(o.stdout.is_empty());
    let stderr = String::from_utf8(o.stderr).unwrap();
    assert!(stderr.contains("terms[0]"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!stderr.contains("aaaa"), "{stderr}");
}

#[test]
fn rules_array_exits_1() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(&cfg, "terms = [\"Acme Corp\"]\nrules = [false]\n").unwrap();
    let o = sagiri(dir.path())
        .arg("--config")
        .arg(&cfg)
        .write_stdin("about Acme Corp\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(o.stdout.is_empty());
    let stderr = String::from_utf8(o.stderr).unwrap();
    assert!(stderr.contains("`rules`"), "{stderr}");
    assert!(!stderr.contains("Acme"), "{stderr}");
}

#[test]
fn rule_toggles_apply() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(&cfg, "[rules]\nipv4 = false\n").unwrap();
    let o = out(
        sagiri(dir.path()).arg("--config").arg(&cfg),
        "ip 198.51.100.7 host git.acme.com\n",
    );
    assert!(o.contains("198.51.100.7"), "{o}");
    assert!(!o.contains("git.acme.com"), "{o}");

    fs::write(
        &cfg,
        "terms = [\"Project Falcon\"]\n[rules]\nterm = false\n",
    )
    .unwrap();
    let o = out(
        sagiri(dir.path()).arg("--config").arg(&cfg),
        "about Project Falcon\n",
    );
    assert_eq!(o, "about Project Falcon\n");
}

#[test]
fn works_without_home_when_no_config_is_given() {
    let dir = TempDir::new().unwrap();
    let o = out(
        sagiri(dir.path())
            .env_remove("HOME")
            .env_remove("XDG_CONFIG_HOME"),
        "ip 198.51.100.7\n",
    );
    assert_ne!(o, "ip 198.51.100.7\n");
}
