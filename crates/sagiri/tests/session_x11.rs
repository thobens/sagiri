//! Drives the real `sagiri-session` binary against an X server. Every test is ignored,
//! because the quality gate runs without a display; `scripts/x11-test.sh` runs them under
//! Xvfb, one at a time, since they share one X server and one clipboard.
//!
//! The test process is the application that copies: it sets and reads the clipboard
//! with `arboard`.
//!
//! The tests refuse to run unless `SAGIRI_X11_TEST` is set, which only the script does,
//! so that `--ignored` on a desktop never touches the real clipboard or keyboard.

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
        assert!(
            std::env::var_os("SAGIRI_X11_TEST").is_some(),
            "these tests take over the clipboard and send keys to the X display; \
             run them with scripts/x11-test.sh"
        );
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
                .stderr(std::fs::File::create(session.log()).unwrap())
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

    /// Where the stderr of `run` goes. It is printed only if the test fails.
    fn log(&self) -> PathBuf {
        self.dir.path().join("stderr.log")
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
            if thread::panicking() {
                let log = std::fs::read_to_string(self.log()).unwrap_or_default();
                eprintln!("stderr of sagiri-session run:\n{log}");
            } else {
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

/// A 1x1 grey PNG.
const PNG: [u8; 67] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x3a, 0x7e, 0x9b,
    0x55, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x01, 0x48, 0xaf, 0xa4, 0x71, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// Runs `xclip -o` on the clipboard with `args` and returns its stdout, or `None` if it
/// fails.
fn xclip_out(args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("cannot run xclip");
    out.status.success().then_some(out.stdout)
}

/// Another program that owns the clipboard. It runs in the foreground and is killed when
/// dropped.
struct Owner(Child);

impl Owner {
    /// Starts `program` with `args`, writes `content` to its stdin and waits until the
    /// clipboard lists `target`.
    fn start(program: &str, args: &[&str], content: &[u8], target: &str) -> Self {
        use std::io::Write as _;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|_| panic!("cannot run {program}"));
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(content).unwrap();
        drop(stdin);
        let owner = Owner(child);
        wait_for("the clipboard to list the target", || {
            let targets = String::from_utf8(xclip_out(&["-t", "TARGETS"])?).ok()?;
            targets.lines().any(|t| t == target).then_some(())
        });
        owner
    }

    /// `xclip` serving `content` as `target`. It answers every request with `content`,
    /// whatever target is asked for.
    fn xclip(target: &str, content: &[u8]) -> Self {
        let args = ["-selection", "clipboard", "-t", target, "-i", "-quiet"];
        Self::start("xclip", &args, content, target)
    }

    /// `xsel` serving `text`.
    fn xsel(text: &str) -> Self {
        let args = ["--clipboard", "--input", "--nodetach"];
        Self::start("xsel", &args, text.as_bytes(), "UTF8_STRING")
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `xclip -t image/png` answers a text request with the PNG; the session must still treat
/// the clipboard as holding no text and leave it alone.
#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_image_from_xclip_left_alone() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let _owner = Owner::xclip("image/png", &PNG);
    thread::sleep(QUIET);
    assert_eq!(
        xclip_out(&["-t", "image/png"]).as_deref(),
        Some(&PNG[..]),
        "the image is no longer served"
    );
    let log = std::fs::read_to_string(session.log()).unwrap();
    assert_eq!(log.trim(), "sagiri-session: session started", "{log}");
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_cli_reads_image_from_xclip_as_no_text() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.toml"), "").unwrap();
    let _owner = Owner::xclip("image/png", &PNG);
    let out = Command::new(env!("CARGO_BIN_EXE_sagiri-cli"))
        .env_remove("SAGIRI_KEY_FILE")
        .env_remove("SAGIRI_CONFIG")
        .env_remove("WAYLAND_DISPLAY")
        .arg("--key-file")
        .arg(dir.path().join("key"))
        .arg("--config")
        .arg(dir.path().join("config.toml"))
        .args(["--in", "clipboard"])
        .stdin(Stdio::null())
        .output()
        .expect("cannot run sagiri-cli");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "sagiri-cli: the clipboard holds no text\n"
    );
    assert_eq!(xclip_out(&["-t", "image/png"]).as_deref(), Some(&PNG[..]));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_replaces_text_from_xclip() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let _owner = Owner::xclip("UTF8_STRING", SAMPLE.as_bytes());
    let replaced = Clipboard::new().replaced(SAMPLE);
    assert!(!replaced.contains(SAMPLE_IP));
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_replaces_text_from_xsel() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let _owner = Owner::xsel(SAMPLE);
    let replaced = Clipboard::new().replaced(SAMPLE);
    assert!(!replaced.contains(SAMPLE_IP));
}
