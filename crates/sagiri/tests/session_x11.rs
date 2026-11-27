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

/// Panics unless `SAGIRI_X11_TEST` is set. Everything that takes over the clipboard or
/// the keyboard calls this first.
fn require_x11_test() {
    assert!(
        std::env::var_os("SAGIRI_X11_TEST").is_some(),
        "these tests take over the clipboard and send keys to the X display; \
         run them with scripts/x11-test.sh"
    );
}

/// The clipboard as an application sees it.
struct Clipboard(arboard::Clipboard);

impl Clipboard {
    fn new() -> Self {
        require_x11_test();
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
        require_x11_test();
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
        require_x11_test();
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

/// How `TextOwner` handles a `TARGETS` request.
#[derive(Clone, Copy)]
enum TargetsReply {
    /// Answers with property `None`, as for a target it does not offer.
    Refuse,
    /// Sends no answer at all.
    Ignore,
}

/// A clipboard owner that serves `text` as `UTF8_STRING`, refuses every other target
/// and handles `TARGETS` as `reply` says. Returns once it owns the clipboard; its thread
/// ends when another program takes the clipboard over, or after `DEADLINE`.
fn text_owner(text: &'static str, reply: TargetsReply) -> thread::JoinHandle<()> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
        SelectionNotifyEvent, WindowClass,
    };
    use x11rb::wrapper::ConnectionExt as _;

    require_x11_test();
    let (conn, screen) = x11rb::connect(None).expect("cannot connect to the X server");
    let atom = |name: &[u8]| conn.intern_atom(false, name).unwrap().reply().unwrap().atom;
    let (clipboard, utf8, targets) = (atom(b"CLIPBOARD"), atom(b"UTF8_STRING"), atom(b"TARGETS"));
    let window = conn.generate_id().unwrap();
    let root = conn.setup().roots[screen].root;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .unwrap();
    conn.set_selection_owner(window, clipboard, x11rb::CURRENT_TIME)
        .unwrap();
    let owner = conn
        .get_selection_owner(clipboard)
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(owner.owner, window, "cannot take the clipboard");

    thread::spawn(move || {
        let deadline = Instant::now() + DEADLINE;
        while Instant::now() < deadline {
            let event = match conn.poll_for_event().unwrap() {
                Some(event) => event,
                None => {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
            };
            let request = match event {
                Event::SelectionRequest(request) => request,
                Event::SelectionClear(_) => return,
                _ => continue,
            };
            let property = if request.target == utf8 {
                conn.change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    request.property,
                    utf8,
                    text.as_bytes(),
                )
                .unwrap();
                request.property
            } else if request.target == targets && matches!(reply, TargetsReply::Ignore) {
                continue;
            } else {
                x11rb::NONE
            };
            let notify = SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: request.time,
                requestor: request.requestor,
                selection: request.selection,
                target: request.target,
                property,
            };
            conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)
                .unwrap();
            conn.flush().unwrap();
        }
    })
}

/// An owner that refuses `TARGETS` counts as offering text: its text is replaced.
#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_replaces_text_from_owner_refusing_targets() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let owner = text_owner(SAMPLE, TargetsReply::Refuse);
    let replaced = Clipboard::new().replaced(SAMPLE);
    assert!(!replaced.contains(SAMPLE_IP));
    owner.join().unwrap();
}

/// An owner that never answers `TARGETS` counts as offering text: its text is replaced
/// once the query has timed out.
#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_replaces_text_from_owner_ignoring_targets() {
    let session = Session::start("");
    assert_eq!(session.send("start"), "active");
    let owner = text_owner(SAMPLE, TargetsReply::Ignore);
    let replaced = Clipboard::new().replaced(SAMPLE);
    assert!(!replaced.contains(SAMPLE_IP));
    owner.join().unwrap();
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

/// The colours of the tray icon, as `src/session/tray.rs` defines them.
const TRAY_ACTIVE: (u8, u8, u8) = (0x2f, 0xb3, 0x44);
const TRAY_INACTIVE: (u8, u8, u8) = (0x90, 0x90, 0x90);

/// The side that the test tray gives an icon.
const TRAY_SIDE: u16 = 24;

/// The background of the test tray's window. Not 0, so that a transparent icon
/// background and one that shows the tray differ.
const TRAY_BACKGROUND: (u8, u8, u8) = (0x20, 0x40, 0x60);

/// A minimal system tray: it owns `_NET_SYSTEM_TRAY_S<screen>`, announces itself and
/// embeds an icon window in its own window, which covers the top left corner of the
/// screen. Dropping it closes its connection, which ends the tray like a crash would.
struct TrayManager {
    conn: x11rb::rust_connection::RustConnection,
    screen: usize,
    root: u32,
    /// The selection owner and the parent of embedded icons.
    window: u32,
    selection: u32,
    opcode: u32,
    xembed_info: u32,
    /// The pixel value of `TRAY_BACKGROUND` on `visual`.
    background: u32,
    /// The visual of `window` and of the icons, and its depth.
    visual: u32,
    depth: u8,
    colormap: u32,
}

impl TrayManager {
    /// A tray on the root visual that does not name a visual, as old trays do.
    fn start() -> Self {
        Self::start_on(false)
    }

    /// With `argb`, a tray on the screen's 32-bit visual that names it in
    /// `_NET_SYSTEM_TRAY_VISUAL`, as trays with a translucent bar do.
    fn start_on(argb: bool) -> Self {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{
            AtomEnum, ColormapAlloc, ConnectionExt as _, CreateWindowAux, PropMode, VisualClass,
            WindowClass,
        };
        use x11rb::wrapper::ConnectionExt as _;

        require_x11_test();
        let (conn, screen) = x11rb::connect(None).expect("cannot connect to the X server");
        let atom = |name: &str| {
            conn.intern_atom(false, name.as_bytes())
                .unwrap()
                .reply()
                .unwrap()
                .atom
        };
        let selection = atom(&format!("_NET_SYSTEM_TRAY_S{screen}"));
        let (opcode, xembed_info) = (atom("_NET_SYSTEM_TRAY_OPCODE"), atom("_XEMBED_INFO"));
        let root = &conn.setup().roots[screen];
        let root_window = root.root;
        let (visual, depth, colormap) = if argb {
            let visual = root
                .allowed_depths
                .iter()
                .filter(|d| d.depth == 32)
                .flat_map(|d| &d.visuals)
                .find(|v| v.class == VisualClass::TRUE_COLOR)
                .expect("the X server has no 32-bit visual")
                .visual_id;
            let colormap = conn.generate_id().unwrap();
            conn.create_colormap(ColormapAlloc::NONE, colormap, root.root, visual)
                .unwrap();
            (visual, 32, colormap)
        } else {
            (root.root_visual, root.root_depth, root.default_colormap)
        };
        let wide = |c: u8| u16::from(c) * 257;
        let (red, green, blue) = TRAY_BACKGROUND;
        let background = conn
            .alloc_color(colormap, wide(red), wide(green), wide(blue))
            .unwrap()
            .reply()
            .unwrap()
            .pixel;
        assert_ne!(background, 0);
        let window = conn.generate_id().unwrap();
        conn.create_window(
            depth,
            window,
            root.root,
            0,
            0,
            200,
            40,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(background)
                .border_pixel(0)
                .colormap(colormap),
        )
        .unwrap()
        .check()
        .expect("cannot create the tray window");
        if argb {
            conn.change_property32(
                PropMode::REPLACE,
                window,
                atom("_NET_SYSTEM_TRAY_VISUAL"),
                AtomEnum::VISUALID,
                &[visual],
            )
            .unwrap();
        }
        conn.map_window(window).unwrap();
        conn.set_selection_owner(window, selection, x11rb::CURRENT_TIME)
            .unwrap();
        let owner = conn
            .get_selection_owner(selection)
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(owner.owner, window, "cannot take the tray selection");
        let manager = TrayManager {
            conn,
            screen,
            root: root_window,
            window,
            selection,
            opcode,
            xembed_info,
            background,
            visual,
            depth,
            colormap,
        };
        manager.announce();
        manager
    }

    /// The atom called `name`.
    fn atom(&self, name: &str) -> u32 {
        use x11rb::protocol::xproto::ConnectionExt as _;

        self.conn
            .intern_atom(false, name.as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom
    }

    /// Tells the clients on the root window that this tray owns the selection, with a
    /// `MANAGER` client message, as a tray does when it starts.
    fn announce(&self) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask};

        let announcement = ClientMessageEvent::new(
            32,
            self.root,
            self.atom("MANAGER"),
            [x11rb::CURRENT_TIME, self.selection, self.window, 0, 0],
        );
        self.conn
            .send_event(false, self.root, EventMask::STRUCTURE_NOTIFY, announcement)
            .unwrap();
        self.conn.flush().unwrap();
    }

    /// The window of the next dock request, or `None` if none arrives within `patience`.
    fn dock_request(&self, patience: Duration) -> Option<u32> {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::Event;

        let deadline = Instant::now() + patience;
        loop {
            match self.conn.poll_for_event().unwrap() {
                Some(Event::ClientMessage(message))
                    if message.type_ == self.opcode && message.format == 32 =>
                {
                    let data = message.data.as_data32();
                    // Opcode 0 is SYSTEM_TRAY_REQUEST_DOCK.
                    assert_eq!(data[1], 0, "not a dock request");
                    return Some(data[2]);
                }
                Some(_) => {}
                None if Instant::now() < deadline => thread::sleep(POLL),
                None => return None,
            }
        }
    }

    /// The window of the next dock request. Panics if none arrives.
    fn docked(&self) -> u32 {
        self.dock_request(DEADLINE)
            .expect("no dock request arrived")
    }

    /// Puts `icon` into the tray window at its top left corner, as a tray does. With
    /// `keep`, the icon also survives the end of the tray, as with i3bar.
    fn embed(&self, icon: u32, keep: bool) {
        self.embed_sized(icon, keep, TRAY_SIDE, TRAY_SIDE);
    }

    /// Like `embed`, with the icon `width` by `height` pixels large.
    fn embed_sized(&self, icon: u32, keep: bool, width: u16, height: u16) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ConnectionExt as _, SetMode};

        if keep {
            self.conn.change_save_set(SetMode::INSERT, icon).unwrap();
        }
        self.conn
            .reparent_window(icon, self.window, 0, 0)
            .unwrap()
            .check()
            .expect("cannot reparent the icon");
        self.resize(icon, width, height);
        self.conn
            .map_window(icon)
            .unwrap()
            .check()
            .expect("cannot map the icon");
        self.conn.flush().unwrap();
    }

    /// Gives `icon` a new size, as a tray does when its bar changes.
    fn resize(&self, icon: u32, width: u16, height: u16) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt as _};

        self.conn
            .configure_window(
                icon,
                &ConfigureWindowAux::new()
                    .width(u32::from(width))
                    .height(u32::from(height)),
            )
            .unwrap()
            .check()
            .expect("cannot resize the icon");
        self.conn.flush().unwrap();
    }

    /// Lets go of `icon` without destroying it: hands it back to the root window, as a
    /// tray does that drops an icon.
    fn release(&self, icon: u32) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as _;

        self.conn
            .reparent_window(icon, self.root, 0, 0)
            .unwrap()
            .check()
            .expect("cannot hand the icon back to the root window");
        self.conn.flush().unwrap();
    }

    /// The type and the bytes of the 8-bit property `name` of `window`.
    fn property8(&self, window: u32, name: &str) -> (u32, Vec<u8>) {
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

        let reply = self
            .conn
            .get_property(false, window, self.atom(name), AtomEnum::ANY, 0, 64)
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(reply.format, 8, "{name} is not an 8-bit property");
        (reply.type_, reply.value)
    }

    /// Whether `window` is override-redirect.
    fn override_redirect(&self, window: u32) -> bool {
        use x11rb::protocol::xproto::ConnectionExt as _;

        self.conn
            .get_window_attributes(window)
            .unwrap()
            .reply()
            .unwrap()
            .override_redirect
    }

    /// The pixel value that the background of an icon in this tray should have:
    /// transparent on a 32-bit visual, the tray's own background (`ParentRelative`)
    /// otherwise.
    fn icon_background(&self) -> u32 {
        if self.depth == 32 { 0 } else { self.background }
    }

    /// The value of `_XEMBED_INFO` on `icon`.
    fn xembed_info(&self, icon: u32) -> Vec<u32> {
        use x11rb::protocol::xproto::ConnectionExt as _;

        self.conn
            .get_property(false, icon, self.xembed_info, self.xembed_info, 0, 2)
            .unwrap()
            .reply()
            .unwrap()
            .value32()
            .map(Iterator::collect)
            .unwrap_or_default()
    }

    /// Whether `window` still exists.
    fn exists(&self, window: u32) -> bool {
        use x11rb::protocol::xproto::ConnectionExt as _;

        self.conn
            .get_window_attributes(window)
            .unwrap()
            .reply()
            .is_ok()
    }

    /// The depth and the visual of `window`.
    fn depth_and_visual(&self, window: u32) -> (u8, u32) {
        use x11rb::protocol::xproto::ConnectionExt as _;

        let depth = self
            .conn
            .get_geometry(window)
            .unwrap()
            .reply()
            .unwrap()
            .depth;
        let attributes = self
            .conn
            .get_window_attributes(window)
            .unwrap()
            .reply()
            .unwrap();
        (depth, attributes.visual)
    }

    /// The pixel value that the icon uses for `color`: what the server allocates, with
    /// the opacity bits of a 32-bit visual set.
    fn pixel(&self, color: (u8, u8, u8)) -> u32 {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as _;

        let wide = |c: u8| u16::from(c) * 257;
        let pixel = self
            .conn
            .alloc_color(self.colormap, wide(color.0), wide(color.1), wide(color.2))
            .unwrap()
            .reply()
            .unwrap()
            .pixel;
        let alpha = if self.depth == 32 {
            let visual = self.conn.setup().roots[self.screen]
                .allowed_depths
                .iter()
                .flat_map(|d| &d.visuals)
                .find(|v| v.visual_id == self.visual)
                .unwrap();
            !(visual.red_mask | visual.green_mask | visual.blue_mask)
        } else {
            0
        };
        pixel | alpha
    }

    /// How many pixels of the embedded `icon` have the value `pixel`, for an icon of the
    /// size that `embed` gives.
    fn count(&self, icon: u32, pixel: u32) -> usize {
        self.count_in(icon, TRAY_SIDE, TRAY_SIDE, pixel)
    }

    /// How many pixels of the embedded `icon`, which is `width` by `height` pixels large,
    /// have the value `pixel`.
    fn count_in(&self, icon: u32, width: u16, height: u16, pixel: u32) -> usize {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder};

        let setup = self.conn.setup();
        let image = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, icon, 0, 0, width, height, !0)
            .unwrap()
            .reply()
            .expect("cannot read the icon window");
        let format = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == image.depth)
            .unwrap();
        let bytes = usize::from(format.bits_per_pixel) / 8;
        let pad = usize::from(format.scanline_pad);
        let row = (usize::from(width) * usize::from(format.bits_per_pixel)).div_ceil(pad) * pad / 8;
        let lsb_first = setup.image_byte_order == ImageOrder::LSB_FIRST;
        image
            .data
            .chunks(row)
            .flat_map(|line| line.chunks(bytes).take(usize::from(width)))
            .filter(|raw| {
                let value = raw.iter().enumerate().fold(0u32, |value, (i, &byte)| {
                    let shift = if lsb_first { i } else { bytes - 1 - i };
                    value | (u32::from(byte) << (8 * shift))
                });
                value == pixel
            })
            .count()
    }
}

/// How many pixels the bars of the icon cover in a window of `width` by `height`.
fn bar_area(width: u16, height: u16, active: bool) -> usize {
    sagiri::session::tray::icon(width, height, active)
        .bars
        .iter()
        .map(|bar| usize::from(bar.width) * usize::from(bar.height))
        .sum()
}

/// Clicks with mouse `button` in the middle of an icon embedded by `TrayManager`.
fn click_icon(button: u8) {
    let middle = (TRAY_SIDE / 2).to_string();
    let status = Command::new("xdotool")
        .args(["mousemove", &middle, &middle, "click", &button.to_string()])
        .status()
        .expect("cannot run xdotool");
    assert!(status.success(), "xdotool failed");
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_docks_with_running_manager() {
    let manager = TrayManager::start();
    let _session = Session::start("");
    let icon = manager.docked();
    // Protocol version 0 and the flag XEMBED_MAPPED.
    assert_eq!(manager.xembed_info(icon), [0, 1]);
    // Without `_NET_SYSTEM_TRAY_VISUAL` the icon uses the root visual.
    assert_eq!(
        manager.depth_and_visual(icon),
        (manager.depth, manager.visual)
    );
    // No window manager may take the window while it is not embedded.
    assert!(manager.override_redirect(icon));
    let string = u32::from(x11rb::protocol::xproto::AtomEnum::STRING);
    let utf8 = manager.atom("UTF8_STRING");
    assert_eq!(
        manager.property8(icon, "WM_NAME"),
        (string, b"Sagiri".to_vec())
    );
    assert_eq!(
        manager.property8(icon, "_NET_WM_NAME"),
        (utf8, b"Sagiri".to_vec())
    );
    // Instance, then class.
    assert_eq!(
        manager.property8(icon, "WM_CLASS"),
        (string, b"sagiri-session\0Sagiri\0".to_vec())
    );
    // One icon, one request.
    assert_eq!(manager.dock_request(QUIET), None);
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_docks_with_later_manager() {
    let _session = Session::start("");
    thread::sleep(QUIET);
    let manager = TrayManager::start();
    let icon = manager.docked();
    assert_eq!(manager.xembed_info(icon), [0, 1]);
    assert_eq!(manager.dock_request(QUIET), None);
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_click_toggles() {
    let manager = TrayManager::start();
    let session = Session::start("");
    manager.embed(manager.docked(), false);
    assert_eq!(session.status().as_deref(), Some("inactive"));
    click_icon(1);
    wait_for("status active", || {
        session.status().filter(|s| s == "active")
    });
    // Other buttons do nothing. Checked after each click, since two toggles would
    // cancel out.
    for button in [3, 2] {
        click_icon(button);
        thread::sleep(QUIET);
        assert_eq!(
            session.status().as_deref(),
            Some("active"),
            "button {button}"
        );
    }
    click_icon(1);
    wait_for("status inactive", || {
        session.status().filter(|s| s == "inactive")
    });
}

/// Starts a session next to `manager`, embeds its icon and checks that the icon follows
/// the state of the session.
fn icon_follows_state(manager: &TrayManager) {
    let session = Session::start("");
    let icon = manager.docked();
    manager.embed(icon, false);
    let (on, off) = (manager.pixel(TRAY_ACTIVE), manager.pixel(TRAY_INACTIVE));
    assert_ne!(on, off);

    let side = usize::from(TRAY_SIDE);
    let background = manager.icon_background();
    assert!(background != on && background != off);

    let thin = wait_for("the inactive icon", || {
        Some(manager.count(icon, off)).filter(|&n| n > 0)
    });
    assert_eq!(thin, bar_area(TRAY_SIDE, TRAY_SIDE, false));
    assert_eq!(manager.count(icon, on), 0);
    // Around the bars the tray shows through, or the icon is transparent.
    assert_eq!(manager.count(icon, background), side * side - thin);

    assert_eq!(session.send("start"), "active");
    let thick = wait_for("the active icon", || {
        Some(manager.count(icon, on)).filter(|&n| n > 0)
    });
    assert_eq!(thick, bar_area(TRAY_SIDE, TRAY_SIDE, true));
    assert_eq!(manager.count(icon, off), 0);
    // The states differ in shape too: the active bars are thicker.
    assert!(thick > thin, "{thick} active pixels, {thin} inactive ones");
    assert!(thick < side * side / 2, "{thick} of {} pixels", side * side);
    assert_eq!(manager.count(icon, background), side * side - thick);

    assert_eq!(session.send("stop"), "inactive");
    wait_for("the inactive icon again", || {
        (manager.count(icon, on) == 0 && manager.count(icon, off) == thin).then_some(())
    });
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_icon_shows_state() {
    icon_follows_state(&TrayManager::start());
}

/// A tray with a translucent bar names a 32-bit visual. The icon must use it, with
/// opaque bars on a transparent background.
#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_icon_uses_the_manager_visual() {
    let manager = TrayManager::start_on(true);
    let session = Session::start("");
    let icon = manager.docked();
    assert_eq!(manager.depth_and_visual(icon), (32, manager.visual));
    drop(session);
    icon_follows_state(&manager);
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_redocks_after_manager_restart() {
    for keep in [false, true] {
        let first = TrayManager::start();
        let session = Session::start("");
        let icon = first.docked();
        first.embed(icon, keep);
        // The tray ends. Without `keep` the icon window goes with it; with `keep` the
        // server hands it back to the root window and maps it there.
        drop(first);

        let second = TrayManager::start();
        let again = second.docked();
        assert_ne!(again, icon, "keep: {keep}");
        // The old window is not left behind on the screen.
        wait_for("the old icon window to go", || {
            (!second.exists(icon)).then_some(())
        });
        second.embed(again, false);
        click_icon(1);
        wait_for("status active", || {
            session.status().filter(|s| s == "active")
        });
    }
}

/// A tray that drops the icon hands it back to the root window and keeps running.
#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_redocks_after_release() {
    let manager = TrayManager::start();
    let session = Session::start("");
    let icon = manager.docked();
    manager.embed(icon, false);
    manager.release(icon);
    wait_for("the released icon window to go", || {
        (!manager.exists(icon)).then_some(())
    });
    // The tray that let go is not asked again until it announces itself.
    assert_eq!(manager.dock_request(QUIET), None);
    manager.announce();
    let again = manager.docked();
    assert_ne!(again, icon);
    manager.embed(again, false);
    click_icon(1);
    wait_for("status active", || {
        session.status().filter(|s| s == "active")
    });
}

/// The tray decides the size of the icon, and may change it. The picture follows.
#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_icon_follows_resize() {
    let manager = TrayManager::start();
    let session = Session::start("");
    let icon = manager.docked();
    let (on, off) = (manager.pixel(TRAY_ACTIVE), manager.pixel(TRAY_INACTIVE));
    let background = manager.icon_background();
    // Waits until the icon shows the picture for `width` by `height` in `active`.
    let shows = |width: u16, height: u16, active: bool| {
        let (bars, other) = if active { (on, off) } else { (off, on) };
        let area = bar_area(width, height, active);
        let all = usize::from(width) * usize::from(height);
        wait_for(
            &format!("the icon at {width}x{height}, active {active}"),
            || {
                (manager.count_in(icon, width, height, bars) == area
                    && manager.count_in(icon, width, height, other) == 0
                    && manager.count_in(icon, width, height, background) == all - area)
                    .then_some(())
            },
        );
    };

    // Not the side the icon window starts with.
    manager.embed_sized(icon, false, 40, 40);
    shows(40, 40, false);
    assert_eq!(session.send("start"), "active");
    shows(40, 40, true);
    manager.resize(icon, 48, 30);
    shows(48, 30, true);
    assert_eq!(session.send("stop"), "inactive");
    shows(48, 30, false);
}

#[test]
#[ignore = "needs an X server; run scripts/x11-test.sh"]
fn x11_tray_off_requests_no_dock() {
    let manager = TrayManager::start();
    let _session = Session::start("tray = false\n");
    assert_eq!(manager.dock_request(QUIET), None);
}
