use std::path::Path;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

/// A `sagiri-session` command that never sees the real environment: key, config and
/// runtime locations point into `home`, and no display is reachable.
fn session(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("sagiri-session").unwrap();
    cmd.env_remove("SAGIRI_KEY_FILE")
        .env_remove("SAGIRI_CONFIG")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_RUNTIME_DIR", home.join("run"));
    cmd
}

#[test]
fn no_command_exits_2() {
    let dir = TempDir::new().unwrap();
    session(dir.path())
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains("Usage: sagiri-session"));
}

#[test]
fn client_without_daemon_exits_1() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("s.sock");
    session(dir.path())
        .arg("status")
        .arg("--socket")
        .arg(&socket)
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "sagiri-session: no session process is running; \
             start one with 'sagiri-session run'\n",
        );
}

#[test]
fn missing_runtime_dir_exits_1() {
    let dir = TempDir::new().unwrap();
    session(dir.path())
        .env_remove("XDG_RUNTIME_DIR")
        .arg("status")
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "sagiri-session: XDG_RUNTIME_DIR is not set to an absolute path; \
             use --socket <PATH>\n",
        );
}

#[test]
fn run_without_backend_exits_1() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("s.sock");
    std::fs::write(dir.path().join("config.toml"), "").unwrap();
    session(dir.path())
        .arg("run")
        .arg("--socket")
        .arg(&socket)
        .arg("--key-file")
        .arg(dir.path().join("key"))
        .arg("--config")
        .arg(dir.path().join("config.toml"))
        .assert()
        .code(1)
        .stdout("")
        .stderr("sagiri-session: session mode has no display backend on this platform\n");
    assert!(!socket.exists());
    // The key was created where the flag pointed.
    assert!(dir.path().join("key").exists());
}
