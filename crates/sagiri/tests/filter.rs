use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

/// A command that never sees the real environment: every variable that could locate the
/// real key is replaced by a path inside `home`.
fn sagiri(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("sagiri").unwrap();
    cmd.env_remove("SAGIRI_KEY_FILE")
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"));
    cmd
}

fn run(home: &Path, key: &Path, input: &[u8]) -> Vec<u8> {
    sagiri(home)
        .arg("--key-file")
        .arg(key)
        .write_stdin(input.to_vec())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
}

/// The text between `prefix` and `suffix` in `out`.
fn between<'a>(out: &'a str, prefix: &str, suffix: &str) -> &'a str {
    let rest = out.strip_prefix(prefix).expect("prefix kept");
    rest.strip_suffix(suffix).expect("suffix kept")
}

#[test]
fn reference_example_is_consistent_across_runs() {
    let dir = TempDir::new().unwrap();
    let key = dir.path().join("key");
    let a = String::from_utf8(run(dir.path(), &key, b"server 172.55.0.1 is down")).unwrap();
    let b = String::from_utf8(run(dir.path(), &key, b"ping 172.55.0.1 failed")).unwrap();
    let ra = between(&a, "server ", " is down");
    let rb = between(&b, "ping ", " failed");
    assert_eq!(ra, rb);
    assert_ne!(ra, "172.55.0.1");
    assert!(ra.parse::<std::net::Ipv4Addr>().is_ok());
}

#[test]
fn different_keys_give_different_replacements() {
    let dir = TempDir::new().unwrap();
    let a = run(dir.path(), &dir.path().join("k1"), b"host 172.55.0.1 up\n");
    let b = run(dir.path(), &dir.path().join("k2"), b"host 172.55.0.1 up\n");
    assert_ne!(a, b);
}

#[test]
fn text_without_findings_is_unchanged() {
    let dir = TempDir::new().unwrap();
    let key = dir.path().join("key");
    for input in [
        &b"hello world, nothing to see"[..],
        b"line one\r\nline two\r\n",
        b"trailing newline\n",
        b"\n\n",
    ] {
        assert_eq!(run(dir.path(), &key, input), input);
    }
}

#[test]
fn empty_input_gives_empty_output() {
    let dir = TempDir::new().unwrap();
    assert!(run(dir.path(), &dir.path().join("key"), b"").is_empty());
}

#[test]
fn key_is_created_under_xdg_data_home() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("xdg-data");
    sagiri(dir.path())
        .env("XDG_DATA_HOME", &data)
        .write_stdin("a 172.55.0.1 b\n")
        .assert()
        .success();
    let key = data.join("sagiri/key");
    let mode = fs::metadata(&key).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn env_key_file_is_used() {
    let dir = TempDir::new().unwrap();
    let key = dir.path().join("from-env");
    sagiri(dir.path())
        .env("SAGIRI_KEY_FILE", &key)
        .write_stdin("a 172.55.0.1 b\n")
        .assert()
        .success();
    assert!(key.exists());
    assert!(!dir.path().join("data/sagiri/key").exists());
}

#[test]
fn flag_wins_over_env() {
    let dir = TempDir::new().unwrap();
    let env_key = dir.path().join("from-env");
    let flag_key = dir.path().join("from-flag");
    sagiri(dir.path())
        .env("SAGIRI_KEY_FILE", &env_key)
        .arg("--key-file")
        .arg(&flag_key)
        .write_stdin("a 172.55.0.1 b\n")
        .assert()
        .success();
    assert!(flag_key.exists());
    assert!(!env_key.exists());
}

#[test]
fn invalid_utf8_is_rejected() {
    let dir = TempDir::new().unwrap();
    let out = sagiri(dir.path())
        .arg("--key-file")
        .arg(dir.path().join("key"))
        .write_stdin(vec![b'a', 0xff, 0xfe, b'S', b'E', b'C', b'R', b'E', b'T'])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("UTF-8"));
    assert!(!stderr.contains("SECRET"));
}

#[test]
fn insecure_key_file_is_rejected() {
    let dir = TempDir::new().unwrap();
    let key = dir.path().join("key");
    fs::write(&key, format!("{}\n", "ab".repeat(32))).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    let out = sagiri(dir.path())
        .arg("--key-file")
        .arg(&key)
        .write_stdin("a 172.55.0.1 b\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("sagiri: "));
    assert!(stderr.contains(key.to_str().unwrap()));
}

#[test]
fn large_input() {
    let dir = TempDir::new().unwrap();
    let mut input = String::new();
    let mut i = 0u32;
    while input.len() < 5 * 1024 * 1024 {
        input.push_str(&format!(
            "2026-11-12 10:00:{:02} INFO conn from 172.55.{}.{} port {} ok\n",
            i % 60,
            (i / 250) % 250,
            i % 250 + 1,
            1024 + i % 5000
        ));
        i += 1;
    }
    let out = run(dir.path(), &dir.path().join("key"), input.as_bytes());
    let count = |b: &[u8]| b.iter().filter(|&&c| c == b'\n').count();
    assert_eq!(count(&out), count(input.as_bytes()));
    assert_ne!(out, input.as_bytes());
}
