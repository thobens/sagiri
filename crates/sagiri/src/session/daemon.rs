//! Wires a `Session` to the control socket and to the other event sources, and runs it.
//!
//! Installs no signal handlers itself, so tests can run it in-process.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use super::control::{self, Listener, SocketId};
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
    let id = listener.id();
    let (events, receiver) = mpsc::channel();
    // On error, `listener` is dropped here and removes the socket file.
    sources(events.clone())?;
    let (done_tx, done) = mpsc::channel();
    thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            // `serve` drops the listener, and with it removes the socket file, before
            // `done` is signalled.
            control::serve(listener, events);
            let _ = done_tx.send(());
        })?;
    if start_active {
        session.set_active(true);
    }
    let exit = run_loop(&mut session, &receiver);
    drop(receiver);
    stop_control(&socket, id, &done);
    Ok(exit)
}

/// Longest wait for the control thread to end. Normally it needs at most about a second:
/// it may be reading from a client that sends nothing (one-second read timeout), or
/// writing a reply (one-second write timeout, plus 0.1 s to discard an oversized request).
/// Several silent clients queued ahead of the wake-up request each take a second, so the
/// wait can be exceeded. `run` then returns with the socket file still in place; the
/// process exits and leaves a stale file, which the next `Listener::bind` replaces.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Ends the control thread, which removes the socket file as it ends. With the event
/// receiver gone, the next request it handles makes it stop, so one is sent here.
///
/// If the socket file was deleted or replaced by another instance, the thread cannot be
/// reached. It is then left blocked, ends with the process, and the file at the path is
/// not touched. The wait is bounded either way, so this never hangs.
fn stop_control(socket: &Path, id: Option<SocketId>, done: &Receiver<()>) {
    if id.is_some_and(|id| control::wake(socket, id)) {
        let _ = done.recv_timeout(STOP_TIMEOUT);
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

    /// The socket file is deleted while the daemon runs and something else takes its
    /// path: another instance's socket, or an unrelated file. Ending the daemon then
    /// neither hangs nor removes what is at the path.
    #[test]
    fn replaced_socket_is_left_alone_on_exit() {
        for live_socket in [true, false] {
            let dir = TempDir::new().unwrap();
            let socket = dir.path().join("s.sock");
            let session = Session::new(
                engine(),
                Box::new(FakeSelections::default()),
                Box::new(FakeNotifier::default()),
            );
            let (stash, stashed) = mpsc::channel::<Sender<Event>>();
            let (finished, wait_finished) = mpsc::channel::<()>();
            let client_socket = socket.clone();
            let client = thread::spawn(move || {
                assert!(!send(&client_socket, Command::Status));
                // Created next to the socket and renamed over it, so the replacement's
                // inode is allocated while the original still exists and cannot reuse
                // its number. The identity check in `wake` then always sees a
                // different file.
                let side = client_socket.with_file_name("o.sock");
                let other = if live_socket {
                    Some(std::os::unix::net::UnixListener::bind(&side).unwrap())
                } else {
                    std::fs::write(&side, "x").unwrap();
                    None
                };
                std::fs::rename(&side, &client_socket).unwrap();
                // Quit through the event channel: the socket is no longer reachable.
                let events = stashed.recv().unwrap();
                let (reply, _answer) = mpsc::channel();
                events
                    .send(Event::Command {
                        command: Command::Quit,
                        reply,
                    })
                    .unwrap();
                // Keep the other socket alive until the daemon has finished.
                let _ = wait_finished.recv_timeout(Duration::from_secs(30));
                drop(other);
            });
            let started = Instant::now();
            let exit = run(Parts {
                session,
                socket: socket.clone(),
                start_active: false,
                sources: Box::new(move |events| {
                    stash.send(events).unwrap();
                    Ok(())
                }),
            })
            .unwrap();
            assert_eq!(exit, Exit::Quit);
            // Clearly below the 3 s `STOP_TIMEOUT`: `stop_control` did not wait at all,
            // so `wake` saw the replacement and sent nothing.
            let waited = started.elapsed();
            assert!(waited < Duration::from_millis(1500), "{waited:?}");
            assert!(socket.exists(), "live socket: {live_socket}");
            if !live_socket {
                assert_eq!(std::fs::read(&socket).unwrap(), b"x");
            }
            finished.send(()).unwrap();
            client.join().unwrap();
        }
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(Exit::Quit), 0);
        assert_eq!(exit_code(Exit::Shutdown), 0);
        assert_eq!(exit_code(Exit::BackendLost), 1);
    }
}
