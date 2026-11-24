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

/// The bound control socket. Dropping it removes the socket file.
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
    /// Device and inode of the socket file this listener created, so that drop never
    /// removes a file that has since been replaced.
    id: (u64, u64),
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
        // From here on, dropping `listener` removes the file again.
        // (0, 0) until the file is inspected: drop then removes whatever is at the path.
        let mut listener = Self {
            inner,
            path: path.to_path_buf(),
            id: (0, 0),
        };
        let meta = fs::symlink_metadata(path).map_err(io_error("inspect", path))?;
        listener.id = (meta.dev(), meta.ino());
        fs::set_permissions(path, Permissions::from_mode(0o600))
            .map_err(io_error("set permissions of", path))?;
        Ok(listener)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let ours = fs::symlink_metadata(&self.path)
            .is_ok_and(|m| self.id == (0, 0) || (m.dev(), m.ino()) == self.id);
        if ours {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Answers control requests, one connection at a time, until the session loop is gone
/// (`events` or a reply channel is closed). Then the listener is dropped and the socket
/// file removed.
pub fn serve(listener: Listener, events: Sender<Event>) {
    for stream in listener.inner.incoming() {
        let Ok(stream) = stream else {
            // For example too many open files; do not spin.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if handle(stream, &events).is_break() {
            return;
        }
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
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut buf = [0u8; MAX_REQUEST + 1];
    let mut len = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReadError::Timeout);
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| ReadError::Io)?;
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
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(ReadError::Timeout);
            }
            Err(_) => return Err(ReadError::Io),
        }
    }
    Ok(buf[..len].to_vec())
}

/// Handles one connection. `Break` means the session loop is gone.
fn handle(mut stream: UnixStream, events: &Sender<Event>) -> ControlFlow<()> {
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
                    if events.send(Event::Command { command, reply: tx }).is_err() {
                        return ControlFlow::Break(());
                    }
                    match rx.recv_timeout(REPLY_TIMEOUT) {
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
        Err(e) => return Err(e.into()),
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
        assert!(waited < Duration::from_millis(2500), "{waited:?}");
        let mut reply = String::new();
        silent.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "error timeout\n");
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

    #[test]
    fn stale_socket_is_replaced() {
        let (_dir, path) = socket();
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
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
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let err = request(&path, Command::Status).unwrap_err();
        assert!(matches!(err, RequestError::NotRunning));
        assert_eq!(
            err.to_string(),
            "no session process is running; start one with 'sagiri-session run'"
        );
    }
}
