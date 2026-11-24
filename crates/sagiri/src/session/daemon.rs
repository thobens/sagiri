//! Wires a `Session` to the control socket and to the other event sources, and runs it.
//!
//! Installs no signal handlers itself, so tests can run it in-process.

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use super::control::{self, Listener};
use super::core::{Event, Exit, Session, run_loop};

/// Everything `run` needs.
pub struct Parts {
    pub session: Session,
    /// Where the control socket goes.
    pub socket: PathBuf,
    /// Start the session before the first event.
    pub start_active: bool,
    /// Starts every event source other than the control socket.
    pub sources: Box<dyn FnOnce(Sender<Event>) -> anyhow::Result<()>>,
}

/// Binds the control socket, starts the event sources and handles events until the
/// session loop ends. Afterwards the socket file is gone.
pub fn run(parts: Parts) -> anyhow::Result<Exit> {
    let Parts {
        mut session,
        socket,
        start_active,
        sources,
    } = parts;
    let listener = Listener::bind(&socket)?;
    let (events, receiver) = mpsc::channel();
    // On error, `listener` is dropped here and removes the socket file.
    sources(events.clone())?;
    let control = thread::Builder::new()
        .name("control".into())
        .spawn(move || control::serve(listener, events))?;
    if start_active {
        session.set_active(true);
    }
    let exit = run_loop(&mut session, &receiver);
    drop(receiver);
    stop_control(&socket, control);
    Ok(exit)
}

/// Ends the control thread, which removes the socket file as it ends. With the event
/// receiver gone, the next request it handles makes it stop, so one is sent here.
fn stop_control(socket: &Path, control: JoinHandle<()>) {
    match UnixStream::connect(socket) {
        Ok(mut stream) => {
            let _ = stream.write_all(b"status\n");
            drop(stream);
            let _ = control.join();
        }
        // The socket is unreachable, so the thread cannot be woken. Remove the file in
        // its place; the thread ends with the process.
        Err(_) => {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// The process exit code for the way the session ended.
pub fn exit_code(exit: Exit) -> u8 {
    match exit {
        Exit::Quit | Exit::Shutdown => 0,
        Exit::BackendLost => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{Content, FakeSelections, Selection};
    use crate::session::control::{RequestError, request};
    use crate::session::core::Command;
    use crate::session::notify::{FakeNotifier, Notice};
    use sagiri_core::engine::Engine;
    use sagiri_core::key::Key;
    use sagiri_core::pseudo::Pseudonymizer;
    use sagiri_core::rules::default_rules;
    use std::array;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    const SAMPLE: &str = "server 172.55.0.1 is down";

    fn engine() -> Engine {
        let bytes: [u8; 32] = array::from_fn(|i| i as u8);
        Engine::new(Pseudonymizer::new(&Key::from_bytes(bytes)), default_rules())
    }

    fn no_sources() -> Box<dyn FnOnce(Sender<Event>) -> anyhow::Result<()>> {
        Box::new(|_| Ok(()))
    }

    /// Like `request`, but waits for the daemon to bind its socket first.
    fn send(path: &Path, command: Command) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match request(path, command) {
                Err(RequestError::NotRunning) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => return other.unwrap(),
            }
        }
    }

    struct Outcome {
        exit: Exit,
        replies: Vec<bool>,
        selections: FakeSelections,
        notices: FakeNotifier,
        socket_left: bool,
    }

    /// Runs the daemon on this thread with fakes, and `commands` from a client thread.
    /// The commands must end the run, normally with `quit`.
    fn daemon(start_active: bool, clipboard: Content, commands: Vec<Command>) -> Outcome {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("s.sock");
        let selections = FakeSelections::default();
        selections.set(Selection::Clipboard, clipboard);
        let notices = FakeNotifier::default();
        let session = Session::new(
            engine(),
            Box::new(selections.clone()),
            Box::new(notices.clone()),
        );
        let client_socket = socket.clone();
        let client = thread::spawn(move || {
            commands
                .into_iter()
                .map(|c| send(&client_socket, c))
                .collect::<Vec<_>>()
        });
        let exit = run(Parts {
            session,
            socket: socket.clone(),
            start_active,
            sources: no_sources(),
        })
        .unwrap();
        Outcome {
            exit,
            replies: client.join().unwrap(),
            selections,
            notices,
            socket_left: socket.exists(),
        }
    }

    #[test]
    fn toggle_over_socket_changes_state() {
        use Command::*;
        let out = daemon(false, Content::Other, vec![Toggle, Status, Toggle, Quit]);
        assert_eq!(out.replies, [true, true, false, false]);
        assert_eq!(out.notices.notices(), [Notice::Started, Notice::Stopped]);
    }

    #[test]
    fn start_processes_the_clipboard() {
        use Command::*;
        let out = daemon(
            false,
            Content::Text(SAMPLE.to_string()),
            vec![Status, Start, Quit],
        );
        assert_eq!(out.replies, [false, true, false]);
        let expected = engine().obfuscate(SAMPLE);
        assert_ne!(expected, SAMPLE);
        assert_eq!(
            out.selections.get(Selection::Clipboard),
            Content::Text(expected)
        );
        assert_eq!(out.selections.writes(Selection::Clipboard), 1);
    }

    #[test]
    fn start_active_flag() {
        let out = daemon(true, Content::Other, vec![Command::Status, Command::Quit]);
        assert_eq!(out.replies, [true, false]);
        assert_eq!(out.notices.notices(), [Notice::Started, Notice::Stopped]);
    }

    #[test]
    fn quit_ends_run_and_removes_socket() {
        let out = daemon(false, Content::Other, vec![Command::Quit]);
        assert_eq!(out.exit, Exit::Quit);
        assert_eq!(out.replies, [false]);
        assert!(!out.socket_left);
    }

    #[test]
    fn sources_error_ends_run_early() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("s.sock");
        let selections = FakeSelections::default();
        selections.set(Selection::Clipboard, Content::Text(SAMPLE.to_string()));
        let notices = FakeNotifier::default();
        let session = Session::new(
            engine(),
            Box::new(selections.clone()),
            Box::new(notices.clone()),
        );
        let err = run(Parts {
            session,
            socket: socket.clone(),
            start_active: true,
            sources: Box::new(|_| anyhow::bail!("no source")),
        })
        .err()
        .unwrap();
        assert_eq!(err.to_string(), "no source");
        assert!(!socket.exists());
        // Nothing else happened: the session was never started.
        assert!(notices.notices().is_empty());
        assert_eq!(selections.reads(Selection::Clipboard), 0);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(Exit::Quit), 0);
        assert_eq!(exit_code(Exit::Shutdown), 0);
        assert_eq!(exit_code(Exit::BackendLost), 1);
    }
}
