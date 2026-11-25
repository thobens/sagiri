//! Drives the real `sagiri-session` binary against an X server. Every test is ignored,
//! because the quality gate runs without a display; `scripts/x11-test.sh` runs them under
//! Xvfb, one at a time, since they share one X server and one clipboard.
//!
//! The test process is the application that copies: it sets and reads the clipboard
//! with `arboard`.

#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const SAMPLE: &str = "server 172.55.0.1 is down";
const SAMPLE_IP: &str = "172.55.0.1";

/// Longest wait for something that should happen.
const DEADLINE: Duration = Duration::from_secs(5);
/// How often a condition is checked while waiting.
const POLL: Duration = Duration::from_millis(20);
/// Waiting time for checks that nothing happens. A replacement takes a few milliseconds.
const QUIET: Duration = Duration::from_millis(500);

const BIN: &str = env!("CARGO_BIN_EXE_sagiri-session");

/// Polls `check` until it returns a value, and panics with `what` after `DEADLINE`.
fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(POLL);
    }
}

/// Waits for `child` to exit, and kills it after `DEADLINE`. Returns its exit code, or
/// `None` if it had to be killed or was ended by a signal.
fn wait_exit(child: &mut Child) -> Option<i32> {
    let deadline = Instant::now() + DEADLINE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// The clipboard as an application sees it.
struct Clipboard(arboard::Clipboard);

impl Clipboard {
    fn new() -> Self {
        assert!(
            std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "WAYLAND_DISPLAY is set; run scripts/x11-test.sh"
        );
        Clipboard(arboard::Clipboard::new().expect("cannot open the X11 clipboard"))
    }

    fn copy(&mut self, text: &str) {
        self.0.set_text(text).expect("cannot set the clipboard");
    }

    /// The text on the clipboard, or `None` if it cannot be read right now.
    fn text(&mut self) -> Option<String> {
        self.0.get_text().ok()
    }

    /// Waits until the clipboard holds a text other than `original`, and returns it.
    fn replaced(&mut self, original: &str) -> String {
        wait_for("the clipboard to be replaced", || {
            self.text().filter(|t| t != original)
        })
    }
}

/// A running `sagiri-session run` with its own key, config and socket. Dropping it
/// sends `quit` and waits for the process to end; a process that does not end is killed.
struct Session {
    dir: TempDir,
    child: Option<Child>,
}

impl Session {
    /// Starts a session process with `[session]` set to `notify = false` plus `extra`,
    /// and waits until it answers on its socket.
    fn start(extra: &str) -> Self {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            format!("[session]\nnotify = false\n{extra}"),
        )
        .unwrap();
        let mut session = Session { dir, child: None };
        session.child = Some(
            session
                .run_command()
                .stdout(Stdio::null())
                .spawn()
                .expect("cannot start sagiri-session"),
        );
        wait_for("the session to answer", || {
            if let Some(child) = session.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                panic!("sagiri-session run ended early: {status}");
            }
            session.status()
        });
        session
    }

    fn socket(&self) -> PathBuf {
        self.dir.path().join("s.sock")
    }

    /// `sagiri-session` with this session's socket, and without the user's key, config
    /// or Wayland display.
    fn command(&self) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.env_remove("SAGIRI_KEY_FILE")
            .env_remove("SAGIRI_CONFIG")
            .env_remove("WAYLAND_DISPLAY")
            .arg("--socket")
            .arg(self.socket())
            .stdin(Stdio::null());
        cmd
    }

    /// `sagiri-session run` with this session's socket, key file and config.
    fn run_command(&self) -> Command {
        let mut cmd = self.command();
        cmd.arg("run")
            .arg("--key-file")
            .arg(self.dir.path().join("key"))
            .arg("--config")
            .arg(self.dir.path().join("config.toml"));
        cmd
    }

    fn client(&self, command: &str) -> Output {
        self.command()
            .arg(command)
            .output()
            .expect("cannot run sagiri-session")
    }

    /// What `status` prints, or `None` if the session does not answer.
    fn status(&self) -> Option<String> {
        let out = self.client("status");
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
    }

    /// Sends `command` and returns the reply line.
    fn send(&self, command: &str) -> String {
        let out = self.client(command);
        assert!(out.status.success(), "sagiri-session {command} failed");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = self.client("quit");
            let code = wait_exit(&mut child);
            if !thread::panicking() {
                assert_eq!(code, Some(0), "sagiri-session run did not end cleanly");
            }
        }
    }
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_replaces_copied_text() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let mut clipboard = Clipboard::new();
    clipboard.copy(SAMPLE);
    let replaced = clipboard.replaced(SAMPLE);
    assert!(!replaced.contains(SAMPLE_IP));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_same_text_twice_gets_same_replacement() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let mut clipboard = Clipboard::new();
    clipboard.copy(SAMPLE);
    let first = clipboard.replaced(SAMPLE);
    clipboard.copy(SAMPLE);
    let second = clipboard.replaced(SAMPLE);
    assert_eq!(first, second);
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_does_not_loop() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let mut clipboard = Clipboard::new();
    clipboard.copy(SAMPLE);
    let replaced = clipboard.replaced(SAMPLE);
    thread::sleep(QUIET);
    assert_eq!(clipboard.text().as_deref(), Some(replaced.as_str()));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_inactive_session_leaves_text_alone() {
    let session = Session::start("");
    assert_eq!(session.status().as_deref(), Some("inactive"));
    let mut clipboard = Clipboard::new();
    clipboard.copy(SAMPLE);
    thread::sleep(QUIET);
    assert_eq!(clipboard.text().as_deref(), Some(SAMPLE));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_stop_ends_replacement() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let mut clipboard = Clipboard::new();
    clipboard.copy(SAMPLE);
    clipboard.replaced(SAMPLE);
    assert_eq!(session.send("stop"), "inactive");
    clipboard.copy(SAMPLE);
    thread::sleep(QUIET);
    assert_eq!(clipboard.text().as_deref(), Some(SAMPLE));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_second_instance_refused() {
    let session = Session::start("");
    let mut second = session
        .run_command()
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start sagiri-session");
    assert_eq!(wait_exit(&mut second), Some(1));
    // The first instance still serves its socket.
    assert_eq!(session.status().as_deref(), Some("inactive"));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_hotkey_toggles() {
    let session = Session::start("hotkey = \"ctrl+alt+o\"\n");
    assert_eq!(session.status().as_deref(), Some("inactive"));
    let press = || {
        let status = Command::new("xdotool")
            .args(["key", "ctrl+alt+o"])
            .status()
            .expect("cannot run xdotool");
        assert!(status.success(), "xdotool failed");
    };
    press();
    wait_for("status active", || {
        session.status().filter(|s| s == "active")
    });
    // Presses closer together than 300 ms count as one; this gap keeps the second press
    // apart from the first. The result is still decided by polling below.
    thread::sleep(Duration::from_millis(400));
    press();
    wait_for("status inactive", || {
        session.status().filter(|s| s == "inactive")
    });
}
