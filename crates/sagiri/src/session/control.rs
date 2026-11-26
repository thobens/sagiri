//! The control socket: a one-line protocol over a Unix socket, the server that turns
//! requests into `Event::Command`s, the client used by `sagiri-session start` and the
//! others, and the single-instance check.
//!
//! The socket carries no content. A request is one of five words, a reply is a state or a
//! fixed text, and a request that is not one of the five words is never echoed.

use std::fmt;
use std::fs::{self, DirBuilder, Permissions};
use std::io::{self, ErrorKind, Read as _, Write as _};
use std::ops::ControlFlow;
use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::core::{Command, Event};

/// Longest request line the server accepts, newline included.
const MAX_REQUEST: usize = 64;
/// How long the server waits for a client to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(1);
/// How often the server looks for input on a connection whose read timeout cannot be set.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long the server waits for the session loop to answer. Starting a session reads the
/// clipboard, which can block while the owning application is slow.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a client waits for the reply. Longer than the server's own timeouts together.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);
/// How much of an oversized request the server reads before it closes the connection.
const DISCARD_LIMIT: u64 = 4096;
/// Longest reply a client reads.
const MAX_REPLY: u64 = 256;

const UNKNOWN_COMMAND: &str = "error unknown command";
const TOO_LONG: &str = "error request too long";
const TIMEOUT: &str = "error timeout";

/// The command a request line names, or `None` if it is not exactly one of the five words.
pub fn parse(line: &str) -> Option<Command> {
    match line {
        "start" => Some(Command::Start),
        "stop" => Some(Command::Stop),
        "toggle" => Some(Command::Toggle),
        "status" => Some(Command::Status),
        "quit" => Some(Command::Quit),
        _ => None,
    }
}

/// The word a client sends for `command`.
fn word(command: Command) -> &'static str {
    match command {
        Command::Start => "start",
        Command::Stop => "stop",
        Command::Toggle => "toggle",
        Command::Status => "status",
        Command::Quit => "quit",
    }
}

/// The reply for a session state, without the newline.
pub fn reply_line(active: bool) -> &'static str {
    if active { "active" } else { "inactive" }
}

/// Why `Listener::bind` failed.
#[derive(Debug)]
pub enum BindError {
    /// Another process accepts connections on the socket.
    AlreadyRunning,
    /// Something other than a socket is in the way; it is left alone.
    NotASocket(PathBuf),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("a session process is already running"),
            Self::NotASocket(path) => write!(f, "{} exists and is not a socket", path.display()),
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "cannot {action} {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for BindError {}

fn io_error(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> BindError {
    let path = path.to_path_buf();
    move |source| BindError::Io {
        action,
        path,
        source,
    }
}

/// Identifies the socket file a `Listener` created: device and inode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SocketId(u64, u64);

impl SocketId {
    /// The id of what is at `path` now, if anything.
    fn at(path: &Path) -> Option<Self> {
        fs::symlink_metadata(path)
            .ok()
            .map(|m| Self(m.dev(), m.ino()))
    }
}

/// The bound control socket. Dropping it removes the socket file.
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
    /// The socket file this listener created, so that nothing ever removes a file that
    /// has since been replaced. `None` only while `bind` has not inspected it yet.
    id: Option<SocketId>,
}

impl Listener {
    /// Binds the control socket at `path`, creating its directory (mode 0700) if needed.
    /// A stale socket file from a process that is gone is replaced; a live one is refused.
    pub fn bind(path: &Path) -> Result<Self, BindError> {
        let missing_parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty() && !p.exists());
        if let Some(parent) = missing_parent {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(io_error("create directory", parent))?;
        }
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_socket() => match UnixStream::connect(path) {
                Ok(_) => return Err(BindError::AlreadyRunning),
                Err(e) if e.kind() == ErrorKind::ConnectionRefused => {
                    fs::remove_file(path).map_err(io_error("remove stale socket", path))?;
                }
                Err(e) => return Err(io_error("connect to", path)(e)),
            },
            Ok(_) => return Err(BindError::NotASocket(path.to_path_buf())),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(io_error("inspect", path)(e)),
        }
        let inner = UnixListener::bind(path).map_err(io_error("bind socket", path))?;
        // From here on, dropping `listener` removes the file again. Until the file is
        // inspected, drop removes whatever is at the path: it was bound a moment ago.
        let mut listener = Self {
            inner,
            path: path.to_path_buf(),
            id: None,
        };
        let meta = fs::symlink_metadata(path).map_err(io_error("inspect", path))?;
        listener.id = Some(SocketId(meta.dev(), meta.ino()));
        fs::set_permissions(path, Permissions::from_mode(0o600))
            .map_err(io_error("set permissions of", path))?;
        Ok(listener)
    }

    /// The socket file this listener created.
    pub fn id(&self) -> Option<SocketId> {
        self.id
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let ours = match self.id {
            None => true,
            Some(id) => SocketId::at(&self.path) == Some(id),
        };
        if ours {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Answers control requests, one connection at a time, until the session loop is gone
/// (`events` or a reply channel is closed). Then the listener is dropped and the socket
/// file removed.
pub fn serve(listener: Listener, events: Sender<Event>) {
    serve_with(listener, events, REPLY_TIMEOUT);
}

/// `serve` with an explicit wait for the session loop's answer.
fn serve_with(listener: Listener, events: Sender<Event>, reply_timeout: Duration) {
    for stream in listener.inner.incoming() {
        let Ok(stream) = stream else {
            // For example too many open files; do not spin.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if handle(stream, &events, reply_timeout).is_break() {
            return;
        }
    }
}

/// Makes a `serve` loop whose session loop is gone notice it and end, by sending it one
/// request. Does nothing unless the file at `path` is still the socket `id` names, so a
/// request never reaches another instance. Returns whether the request was sent.
///
/// The check is best effort: the file can be replaced between the check and the
/// connect, and a filesystem that reuses inodes can give a replacement the same id. Then
/// another instance receives one `status` request, which changes nothing, and the caller
/// still waits only for a bounded time.
pub fn wake(path: &Path, id: SocketId) -> bool {
    if SocketId::at(path) != Some(id) {
        return false;
    }
    match UnixStream::connect(path) {
        Ok(mut stream) => stream.write_all(b"status\n").is_ok(),
        Err(_) => false,
    }
}

enum ReadError {
    TooLong,
    Timeout,
    Io,
}

/// Reads one request line within `READ_TIMEOUT` in total. The newline is optional when
/// the client closes its side.
fn read_request(stream: &mut UnixStream) -> Result<Vec<u8>, ReadError> {
    read_request_with(stream, |stream, timeout| {
        stream.set_read_timeout(Some(timeout))
    })
}

/// `read_request` with the call that sets the read timeout, so that a test can make it
/// fail the way macOS does.
fn read_request_with(
    stream: &mut UnixStream,
    set_read_timeout: impl Fn(&UnixStream, Duration) -> io::Result<()>,
) -> Result<Vec<u8>, ReadError> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut buf = [0u8; MAX_REQUEST + 1];
    let mut len = 0;
    // Whether the stream is non-blocking and polled, because no read timeout could be set.
    let mut polling = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReadError::Timeout);
        }
        if !polling && set_read_timeout(stream, remaining).is_err() {
            // macOS refuses every socket option (`EINVAL`) once the peer has closed its
            // end, which `wake` and any client that does not wait for the reply do right
            // after writing. What the peer sent is still there to be read, so the
            // request must not be dropped: a `serve` loop that drops the wake-up request
            // never ends. Without a timeout the read is bounded by polling instead. The
            // stream stays non-blocking for the reply, which is short.
            stream.set_nonblocking(true).map_err(|_| ReadError::Io)?;
            polling = true;
        }
        match stream.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => {
                len += n;
                if let Some(i) = buf[..len].iter().position(|&b| b == b'\n') {
                    if i + 1 > MAX_REQUEST {
                        return Err(ReadError::TooLong);
                    }
                    return Ok(buf[..i].to_vec());
                }
                if len > MAX_REQUEST {
                    return Err(ReadError::TooLong);
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if polling && e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(POLL_INTERVAL.min(remaining));
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(ReadError::Timeout);
            }
            Err(_) => return Err(ReadError::Io),
        }
    }
    Ok(buf[..len].to_vec())
}

/// Handles one connection. `Break` means the session loop is gone.
fn handle(
    mut stream: UnixStream,
    events: &Sender<Event>,
    reply_timeout: Duration,
) -> ControlFlow<()> {
    let reply = match read_request(&mut stream) {
        Err(ReadError::TooLong) => TOO_LONG,
        Err(ReadError::Timeout) => TIMEOUT,
        Err(ReadError::Io) => return ControlFlow::Continue(()),
        Ok(line) => {
            let line = line.strip_suffix(b"\r").unwrap_or(&line);
            match std::str::from_utf8(line).ok().and_then(parse) {
                None => UNKNOWN_COMMAND,
                Some(command) => {
                    let (tx, rx) = mpsc::channel();
                    // The session loop is gone, either before this request was sent or
                    // after it was queued behind one that ended the loop (a `quit` from
                    // another client). Either way the connection closes without a reply,
                    // which the client reports as an invalid reply.
                    if events.send(Event::Command { command, reply: tx }).is_err() {
                        return ControlFlow::Break(());
                    }
                    match rx.recv_timeout(reply_timeout) {
                        Ok(active) => reply_line(active),
                        Err(RecvTimeoutError::Timeout) => TIMEOUT,
                        Err(RecvTimeoutError::Disconnected) => return ControlFlow::Break(()),
                    }
                }
            }
        }
    };
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let _ = stream.write_all(format!("{reply}\n").as_bytes());
    if reply == TOO_LONG {
        discard_rest(&mut stream);
    }
    ControlFlow::Continue(())
}

/// Reads and drops what is left of an oversized request, a bounded amount for a short
/// time. Closing a socket with unread input resets the connection, and the client would
/// lose the reply.
fn discard_rest(stream: &mut UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = io::copy(&mut stream.take(DISCARD_LIMIT), &mut io::sink());
}

/// Why `request` failed.
#[derive(Debug)]
pub enum RequestError {
    /// No socket file, or nothing accepts connections on it.
    NotRunning,
    /// The server answered `error <reason>`.
    Server(String),
    /// The reply was not part of the protocol.
    BadReply,
    /// Connecting failed for another reason, for example a path that is too long.
    Connect {
        path: PathBuf,
        source: io::Error,
    },
    Io(io::Error),
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunning => {
                f.write_str("no session process is running; start one with 'sagiri-session run'")
            }
            Self::Server(reason) => f.write_str(reason),
            Self::BadReply => f.write_str("the session process sent an invalid reply"),
            Self::Connect { path, source } => {
                write!(f, "cannot connect to {}: {source}", path.display())
            }
            Self::Io(e) => write!(f, "cannot talk to the session process: {e}"),
        }
    }
}

impl std::error::Error for RequestError {}

impl From<io::Error> for RequestError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Sends `command` to the session process at `path` and returns the state it reports.
pub fn request(path: &Path, command: Command) -> Result<bool, RequestError> {
    let mut stream = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
            return Err(RequestError::NotRunning);
        }
        Err(source) => {
            return Err(RequestError::Connect {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    stream.write_all(format!("{}\n", word(command)).as_bytes())?;
    let mut reply = String::new();
    stream.take(MAX_REPLY).read_to_string(&mut reply)?;
    match reply.strip_suffix('\n') {
        Some("active") => Ok(true),
        Some("inactive") => Ok(false),
        Some(other) => match other.strip_prefix("error ") {
            Some(reason) => Err(RequestError::Server(reason.to_string())),
            None => Err(RequestError::BadReply),
        },
        None => Err(RequestError::BadReply),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Shutdown;
    use std::thread;
    use tempfile::TempDir;

    const WORDS: [(&str, Command); 5] = [
        ("start", Command::Start),
        ("stop", Command::Stop),
        ("toggle", Command::Toggle),
        ("status", Command::Status),
        ("quit", Command::Quit),
    ];

    /// A socket path inside a fresh temporary directory, short enough for the limit.
    fn socket() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("s.sock");
        (dir, path)
    }

    /// Binds and serves at `path`, with a stand-in for the session loop that answers
    /// every command with `true`.
    fn serving(path: &Path) {
        let listener = Listener::bind(path).unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || serve(listener, tx));
        thread::spawn(move || {
            for event in rx {
                if let Event::Command { reply, .. } = event {
                    let _ = reply.send(true);
                }
            }
        });
    }

    /// Sends raw bytes, closes the writing side and returns the whole reply.
    fn raw(path: &Path, bytes: &[u8]) -> String {
        let mut stream = UnixStream::connect(path).unwrap();
        stream.write_all(bytes).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn command_words_roundtrip() {
        for (w, command) in WORDS {
            assert_eq!(parse(w), Some(command), "{w}");
            assert_eq!(word(command), w);
        }
        assert_eq!(reply_line(true), "active");
        assert_eq!(reply_line(false), "inactive");
    }

    #[test]
    fn unknown_word_is_none() {
        for line in [
            "",
            "pause",
            "status now",
            "Start",
            " stop",
            "toggle ",
            "quit\n",
        ] {
            assert_eq!(parse(line), None, "{line:?}");
        }
    }

    #[test]
    fn unknown_request_gets_error_reply() {
        let (_dir, path) = socket();
        serving(&path);
        let reply = raw(&path, b"launch-codes-1234\n");
        assert_eq!(reply, "error unknown command\n");
        assert!(!reply.contains("launch-codes"));
        // Not UTF-8 either.
        assert_eq!(raw(&path, b"\xff\xfe\n"), "error unknown command\n");
        // The server keeps going, and accepts a request without a newline.
        assert_eq!(raw(&path, b"status"), "active\n");
        assert_eq!(raw(&path, b"status\r\n"), "active\n");
        assert!(matches!(request(&path, Command::Toggle), Ok(true)));
    }

    #[test]
    fn oversized_request_gets_error_reply() {
        let (_dir, path) = socket();
        serving(&path);
        let mut long = vec![b'a'; 200];
        long.push(b'\n');
        assert_eq!(raw(&path, &long), "error request too long\n");
        // 64 bytes with the newline is the limit.
        let mut limit = vec![b'a'; MAX_REQUEST - 1];
        limit.push(b'\n');
        assert_eq!(raw(&path, &limit), "error unknown command\n");
        let mut over = vec![b'a'; MAX_REQUEST];
        over.push(b'\n');
        assert_eq!(raw(&path, &over), "error request too long\n");
        assert_eq!(
            raw(&path, &[b'a'; MAX_REQUEST + 1]),
            "error request too long\n"
        );
        assert!(request(&path, Command::Status).unwrap());
    }

    #[test]
    fn silent_client_times_out() {
        let (_dir, path) = socket();
        serving(&path);
        let mut silent = UnixStream::connect(&path).unwrap();
        // Let the server pick up the silent connection first.
        thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        assert!(request(&path, Command::Status).unwrap());
        let waited = started.elapsed();
        // About 0.9 s: the read timeout minus the head start above.
        assert!(waited < Duration::from_millis(1500), "{waited:?}");
        let mut reply = String::new();
        silent.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "error timeout\n");
    }

    #[test]
    fn slow_session_gets_timeout_reply() {
        let (_dir, path) = socket();
        let listener = Listener::bind(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || serve_with(listener, tx, Duration::from_millis(100)));
        // A session loop that takes the commands but never answers them.
        thread::spawn(move || {
            let mut held = Vec::new();
            for event in rx {
                held.push(event);
            }
        });
        let err = request(&path, Command::Start).unwrap_err();
        assert!(
            matches!(&err, RequestError::Server(r) if r == "timeout"),
            "{err:?}"
        );
        // Printed as the bare reason.
        assert_eq!(err.to_string(), "timeout");
    }

    #[test]
    fn wake_ends_a_serve_loop_without_session() {
        let (_dir, path) = socket();
        let listener = Listener::bind(&path).unwrap();
        let id = listener.id().unwrap();
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let (done_tx, done) = mpsc::channel();
        thread::spawn(move || {
            serve(listener, tx);
            let _ = done_tx.send(());
        });
        assert!(wake(&path, id));
        // A bounded wait instead of a join: a serve loop that misses the request blocks
        // in `accept` for good, and the test then fails instead of hanging the run.
        assert!(
            done.recv_timeout(Duration::from_secs(10)).is_ok(),
            "the serve loop did not end"
        );
        assert!(!path.exists());
    }

    /// Stands in for macOS, which sets no socket option once the peer has closed.
    fn refuse(_: &UnixStream, _: Duration) -> io::Result<()> {
        Err(ErrorKind::InvalidInput.into())
    }

    #[test]
    fn request_is_read_when_the_timeout_cannot_be_set() {
        // The peer has written its request and closed, as `wake` does.
        for (sent, expected) in [("status\n", "status"), ("quit", "quit"), ("", "")] {
            let (mut server, mut client) = UnixStream::pair().unwrap();
            client.write_all(sent.as_bytes()).unwrap();
            drop(client);
            let line = read_request_with(&mut server, refuse).ok();
            assert_eq!(line.as_deref(), Some(expected.as_bytes()), "{sent:?}");
        }
        // A request that arrives later is still read.
        let (mut server, mut client) = UnixStream::pair().unwrap();
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            client.write_all(b"toggle\n").unwrap();
            client
        });
        let line = read_request_with(&mut server, refuse).ok();
        assert_eq!(line.as_deref(), Some(&b"toggle"[..]));
        drop(writer.join().unwrap());
    }

    #[test]
    fn silent_client_times_out_when_the_timeout_cannot_be_set() {
        let (mut server, _client) = UnixStream::pair().unwrap();
        let started = Instant::now();
        let result = read_request_with(&mut server, refuse);
        let waited = started.elapsed();
        assert!(matches!(result, Err(ReadError::Timeout)));
        assert!(waited >= READ_TIMEOUT, "{waited:?}");
        assert!(waited < READ_TIMEOUT * 2, "{waited:?}");
    }

    #[test]
    fn wake_leaves_a_replaced_socket_alone() {
        let (_dir, path) = socket();
        let listener = Listener::bind(&path).unwrap();
        let id = listener.id().unwrap();
        fs::remove_file(&path).unwrap();
        assert!(!wake(&path, id));
        let other = UnixListener::bind(&path).unwrap();
        other.set_nonblocking(true).unwrap();
        assert!(!wake(&path, id));
        // No connection reached the other socket, and dropping the first listener
        // leaves the other's file in place.
        assert!(other.accept().is_err());
        drop(listener);
        assert!(path.exists());
    }

    #[test]
    fn connect_error_names_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("s".repeat(200));
        let err = request(&path, Command::Status).unwrap_err();
        assert!(matches!(err, RequestError::Connect { .. }), "{err:?}");
        let message = err.to_string();
        assert!(
            message.starts_with(&format!("cannot connect to {}: ", path.display())),
            "{message}"
        );
    }

    #[test]
    fn second_instance_is_refused() {
        let (_dir, path) = socket();
        let _first = Listener::bind(&path).unwrap();
        let second = Listener::bind(&path);
        assert!(matches!(second, Err(BindError::AlreadyRunning)));
        assert_eq!(
            second.err().unwrap().to_string(),
            "a session process is already running"
        );
        // The refused attempt left the first socket in place.
        assert!(UnixStream::connect(&path).is_ok());
    }

    /// Names the path at which `stale_socket_helper` binds a listener and closes it again.
    const STALE_SOCKET_ENV: &str = "SAGIRI_TEST_STALE_SOCKET";

    /// Not a test of its own: `leave_stale_socket` runs it in a child process to leave a
    /// stale socket file behind. Without the variable it does nothing.
    #[test]
    fn stale_socket_helper() {
        if let Some(path) = std::env::var_os(STALE_SOCKET_ENV) {
            drop(UnixListener::bind(path).unwrap());
        }
    }

    /// Leaves a socket file at `path` that nothing listens on.
    ///
    /// A child process binds and exits, like a session process that is gone. Had this
    /// process bound the listener, a child started by a concurrent test could hold a copy
    /// of it for a moment, and the stale socket would accept connections.
    fn leave_stale_socket(path: &Path) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session::control::tests::stale_socket_helper"])
            .env(STALE_SOCKET_ENV, path)
            .output()
            .unwrap();
        // The helper's output holds at most the socket path, never pasted text.
        assert!(
            output.status.success(),
            "stale_socket_helper failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(path.exists());
    }

    #[test]
    fn stale_socket_is_replaced() {
        let (_dir, path) = socket();
        leave_stale_socket(&path);
        assert!(UnixStream::connect(&path).is_err());
        let _listener = Listener::bind(&path).unwrap();
        assert!(UnixStream::connect(&path).is_ok());
    }

    #[test]
    fn something_else_at_the_path_is_left_alone() {
        let (_dir, path) = socket();
        fs::write(&path, "x").unwrap();
        assert!(matches!(
            Listener::bind(&path),
            Err(BindError::NotASocket(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), b"x");
    }

    #[test]
    fn socket_directory_is_created_0700() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rt").join("s.sock");
        let _listener = Listener::bind(&path).unwrap();
        assert_eq!(mode(&dir.path().join("rt")), 0o700);
    }

    #[test]
    fn socket_file_is_0600() {
        let (_dir, path) = socket();
        let _listener = Listener::bind(&path).unwrap();
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn dropping_the_listener_removes_the_socket() {
        let (_dir, path) = socket();
        let listener = Listener::bind(&path).unwrap();
        assert!(path.exists());
        drop(listener);
        assert!(!path.exists());
    }

    #[test]
    fn request_without_listener_is_not_running() {
        let (_dir, path) = socket();
        assert!(matches!(
            request(&path, Command::Status),
            Err(RequestError::NotRunning)
        ));
        leave_stale_socket(&path);
        let err = request(&path, Command::Status).unwrap_err();
        assert!(matches!(err, RequestError::NotRunning), "{err:?}");
        assert_eq!(
            err.to_string(),
            "no session process is running; start one with 'sagiri-session run'"
        );
    }
}
