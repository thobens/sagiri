use std::path::Path;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

/// A command that never sees the real environment: every variable that could locate the
/// real key or config is replaced by a path inside `home`. `DISPLAY` and `WAYLAND_DISPLAY`
/// are removed, so every test also shows that the stdin path needs no display and that no
/// test can reach the real clipboard.
fn command(bin: &str, home: &Path) -> Command {
    let mut cmd = Command::cargo_bin(bin).unwrap();
    cmd.env_remove("SAGIRI_KEY_FILE")
        .env_remove("SAGIRI_CONFIG")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"));
    cmd
}

fn sagiri_cli(home: &Path) -> Command {
    command("sagiri-cli", home)
}

fn stdout_of(bin: &str, home: &Path, key: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
    command(bin, home)
        .arg("--key-file")
        .arg(key)
        .args(args)
        .write_stdin(input.to_vec())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
}

#[test]
fn cli_matches_filter_mode() {
    let dir = TempDir::new().unwrap();
    let key = dir.path().join("key");
    for input in [
        &b"server 172.55.0.1 is down\n"[..],
        b"mail alice@git.example.com from 2001:db8::7\r\nline two",
        b"no findings here",
        b"",
    ] {
        let filter = stdout_of("sagiri", dir.path(), &key, &[], input);
        let cli = stdout_of("sagiri-cli", dir.path(), &key, &[], input);
        assert_eq!(cli, filter);
        let explicit = stdout_of("sagiri-cli", dir.path(), &key, &["--in", "stdin"], input);
        assert_eq!(explicit, filter);
    }
}

#[test]
fn trim_newline_flag() {
    let dir = TempDir::new().unwrap();
    let key = dir.path().join("key");
    let full = stdout_of("sagiri-cli", dir.path(), &key, &[], b"host 172.55.0.1\n\n");
    let trimmed = stdout_of(
        "sagiri-cli",
        dir.path(),
        &key,
        &["-n"],
        b"host 172.55.0.1\n\n",
    );
    assert!(full.ends_with(b"\n\n"));
    assert_eq!(trimmed, &full[..full.len() - 1]);

    let long = stdout_of(
        "sagiri-cli",
        dir.path(),
        &key,
        &["--trim-newline"],
        b"x\r\n",
    );
    assert_eq!(long, b"x");
    let none = stdout_of("sagiri-cli", dir.path(), &key, &["-n"], b"x");
    assert_eq!(none, b"x");
}

#[test]
fn invalid_in_value_exits_2() {
    let dir = TempDir::new().unwrap();
    let out = sagiri_cli(dir.path())
        .args(["--in", "printer"])
        .write_stdin("a\n")
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--in"), "{stderr}");
    assert!(!dir.path().join("data/sagiri/key").exists());
}

#[test]
fn invalid_utf8_exits_1() {
    let dir = TempDir::new().unwrap();
    let out = sagiri_cli(dir.path())
        .arg("--key-file")
        .arg(dir.path().join("key"))
        .write_stdin(vec![b'a', 0xff, 0xfe, b'S', b'E', b'C', b'R', b'E', b'T'])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stderr, "sagiri-cli: input is not valid UTF-8\n");
}

/// Runs `bin` with `args`, expects exit 1 and empty stdout, and returns stderr.
fn failure(bin: &str, home: &Path, args: &[&std::ffi::OsStr]) -> String {
    let out = command(bin, home)
        .args(args)
        .write_stdin("server 172.55.0.1 is down\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    String::from_utf8(out.stderr).unwrap()
}

#[test]
fn key_and_config_errors_exit_1_as_in_sagiri() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let key = dir.path().join("key");
    fs::write(&key, format!("{}\n", "ab".repeat(32))).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    let config = dir.path().join("config.toml");
    fs::write(&config, "unknown_setting = true\n").unwrap();
    let good_key = dir.path().join("good-key");

    let cases: [Vec<&std::ffi::OsStr>; 2] = [
        vec!["--key-file".as_ref(), key.as_os_str()],
        vec![
            "--key-file".as_ref(),
            good_key.as_os_str(),
            "--config".as_ref(),
            config.as_os_str(),
        ],
    ];
    for args in cases {
        let cli = failure("sagiri-cli", dir.path(), &args);
        let filter = failure("sagiri", dir.path(), &args);
        let message = cli.strip_prefix("sagiri-cli: ").expect("prefix");
        assert_eq!(Some(message), filter.strip_prefix("sagiri: "), "{cli}");
        assert!(!cli.contains("172.55.0.1"), "{cli}");
    }
}

#[test]
fn help_and_version() {
    let dir = TempDir::new().unwrap();
    sagiri_cli(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--in"))
        .stdout(contains("--trim-newline"))
        .stdout(contains("--key-file"))
        .stdout(contains("--config"));
    sagiri_cli(dir.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("sagiri-cli {}\n", env!("CARGO_PKG_VERSION")));
}

#[cfg(target_os = "linux")]
#[test]
fn clipboard_without_display_exits_1() {
    let dir = TempDir::new().unwrap();
    // `command()` removes `DISPLAY` and `WAYLAND_DISPLAY`, so opening the clipboard fails.
    let out = sagiri_cli(dir.path())
        .arg("--key-file")
        .arg(dir.path().join("key"))
        .args(["--in", "clipboard"])
        .write_stdin("server 172.55.0.1 is down\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.starts_with("sagiri-cli: cannot read the clipboard"),
        "{stderr}"
    );
}
