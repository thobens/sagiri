//! The session state machine: which clipboard changes are replaced, what happens on
//! failure, and how Sagiri avoids processing its own output. Driven by `Event`s, with no
//! knowledge of sockets, signals or the display.
//!
//! Nothing here prints or logs, and no type here has a `Debug` that could show clipboard
//! text.

use std::sync::mpsc::{Receiver, Sender};

use sagiri_core::config::PrimaryMode;
use sagiri_core::engine::Engine;

use super::notify::{Notice, Notifier};
use crate::clipboard::{Content, Selection, SelectionClipboard};

/// A request from the control socket.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Start,
    Stop,
    Toggle,
    Status,
    Quit,
}

/// Something the session loop reacts to.
pub enum Event {
    /// The selection has a new owner.
    Changed(Selection),
    /// A request from the control socket. The reply is the state after the command.
    Command {
        command: Command,
        reply: Sender<bool>,
    },
    /// The hotkey was pressed.
    Toggle,
    BackendLost,
    /// SIGINT or SIGTERM.
    Shutdown,
}

/// What `Session::on_change` did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The session is inactive; the selection was not touched.
    Inactive,
    /// The text was obfuscated and written back.
    Replaced,
    /// The text is what this session last wrote to the selection.
    Own,
    /// Files or other content without text; nothing was written.
    Skipped,
    /// Reading or writing failed; the selection was cleared if possible.
    Failed,
}

/// Why `run_loop` ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    Quit,
    Shutdown,
    BackendLost,
}

/// The last text this session wrote to each selection. It is Sagiri's own output, never
/// an original, and lives in memory only.
#[derive(Default)]
struct LastWritten {
    clipboard: Option<String>,
    primary: Option<String>,
}

impl LastWritten {
    fn slot(&mut self, selection: Selection) -> &mut Option<String> {
        match selection {
            Selection::Clipboard => &mut self.clipboard,
            Selection::Primary => &mut self.primary,
        }
    }
}

/// An obfuscating session. Owns the clipboard and is used from one thread only.
pub struct Session {
    engine: Engine,
    clipboard: Box<dyn SelectionClipboard>,
    notifier: Box<dyn Notifier>,
    active: bool,
    primary: PrimaryMode,
    last_written: LastWritten,
}

impl Session {
    /// A new session. It starts inactive.
    pub fn new(
        engine: Engine,
        clipboard: Box<dyn SelectionClipboard>,
        notifier: Box<dyn Notifier>,
    ) -> Self {
        Self {
            engine,
            clipboard,
            notifier,
            active: false,
            primary: PrimaryMode::Off,
            last_written: LastWritten::default(),
        }
    }

    /// Sets how PRIMARY is treated. The default is `PrimaryMode::Off`. With
    /// `PrimaryMode::Mirror`, every text written to the clipboard is written to PRIMARY
    /// as well. With `PrimaryMode::Watch`, starting the session processes PRIMARY too.
    pub fn with_primary(mut self, mode: PrimaryMode) -> Self {
        self.primary = mode;
        self
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Starts or stops the session. Starting also processes what the clipboard holds, and
    /// with `PrimaryMode::Watch` what PRIMARY holds. Setting the current state again does
    /// nothing.
    pub fn set_active(&mut self, active: bool) {
        if active == self.active {
            return;
        }
        self.active = active;
        if active {
            self.notifier.notify(Notice::Started);
            self.on_change(Selection::Clipboard);
            if self.primary == PrimaryMode::Watch {
                self.on_change(Selection::Primary);
            }
        } else {
            self.notifier.notify(Notice::Stopped);
        }
    }

    /// Reacts to a change of `selection`.
    pub fn on_change(&mut self, selection: Selection) -> Outcome {
        if !self.active {
            return Outcome::Inactive;
        }
        let text = match self.clipboard.read(selection) {
            Ok(Content::Text(t)) => t,
            Ok(Content::Files | Content::Other) => return Outcome::Skipped,
            Err(_) => return self.fail(selection),
        };
        if self.last_written.slot(selection).as_deref() == Some(text.as_str()) {
            return Outcome::Own;
        }
        let out = self.engine.obfuscate(&text);
        if self.clipboard.write_text(selection, &out).is_err() {
            return self.fail(selection);
        }
        if selection == Selection::Clipboard && self.primary == PrimaryMode::Mirror {
            self.mirror(&out);
        }
        *self.last_written.slot(selection) = Some(out);
        Outcome::Replaced
    }

    /// Writes the clipboard's new text to PRIMARY too. A failure is reported but does not
    /// undo the clipboard replacement, which is already correct.
    fn mirror(&mut self, out: &str) {
        if self.clipboard.write_text(Selection::Primary, out).is_ok() {
            self.last_written.primary = Some(out.to_string());
        } else {
            self.notifier.notify(Notice::PrimaryNotUpdated);
        }
    }

    /// Fails closed: clears the selection and tells the user.
    fn fail(&mut self, selection: Selection) -> Outcome {
        let notice = match self.clipboard.clear(selection) {
            Ok(()) => Notice::ClipboardCleared,
            Err(_) => Notice::OriginalStillOnClipboard,
        };
        self.notifier.notify(notice);
        Outcome::Failed
    }

    /// Ends the session because the display connection is gone. Only `BackendLost` is
    /// sent, no `Stopped`, and it is sent whether or not the session was active.
    fn backend_lost(&mut self) {
        self.active = false;
        self.notifier.notify(Notice::BackendLost);
    }
}

/// Handles events until one ends the loop. A closed channel counts as `Shutdown`.
pub fn run_loop(session: &mut Session, events: &Receiver<Event>) -> Exit {
    loop {
        let Ok(event) = events.recv() else {
            session.set_active(false);
            return Exit::Shutdown;
        };
        match event {
            Event::Changed(selection) => {
                session.on_change(selection);
            }
            Event::Command { command, reply } => {
                let exit = match command {
                    Command::Start => {
                        session.set_active(true);
                        None
                    }
                    Command::Stop => {
                        session.set_active(false);
                        None
                    }
                    Command::Toggle => {
                        session.set_active(!session.is_active());
                        None
                    }
                    Command::Status => None,
                    Command::Quit => {
                        session.set_active(false);
                        Some(Exit::Quit)
                    }
                };
                // A client that went away does not need an answer.
                let _ = reply.send(session.is_active());
                if let Some(exit) = exit {
                    return exit;
                }
            }
            Event::Toggle => session.set_active(!session.is_active()),
            Event::BackendLost => {
                session.backend_lost();
                return Exit::BackendLost;
            }
            Event::Shutdown => {
                session.set_active(false);
                return Exit::Shutdown;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::FakeSelections;
    use crate::session::notify::FakeNotifier;
    use Selection::{Clipboard as Clip, Primary};
    use sagiri_core::key::Key;
    use sagiri_core::pseudo::Pseudonymizer;
    use sagiri_core::rules::default_rules;
    use std::array;
    use std::sync::mpsc;

    const SAMPLE: &str = "server 172.55.0.1 is down";

    fn engine() -> Engine {
        let bytes: [u8; 32] = array::from_fn(|i| i as u8);
        Engine::new(Pseudonymizer::new(&Key::from_bytes(bytes)), default_rules())
    }

    fn obfuscated(text: &str) -> String {
        engine().obfuscate(text)
    }

    fn text(s: &str) -> Content {
        Content::Text(s.to_string())
    }

    fn session() -> (Session, FakeSelections, FakeNotifier) {
        let sel = FakeSelections::default();
        let notes = FakeNotifier::default();
        let s = Session::new(engine(), Box::new(sel.clone()), Box::new(notes.clone()));
        (s, sel, notes)
    }

    /// An active session whose activation saw an empty clipboard. The notice record is
    /// left with `Started` only.
    fn active() -> (Session, FakeSelections, FakeNotifier) {
        let (mut s, sel, notes) = session();
        s.set_active(true);
        assert_eq!(notes.notices(), [Notice::Started]);
        (s, sel, notes)
    }

    fn counters(sel: &FakeSelections, selection: Selection) -> (usize, usize, usize) {
        (
            sel.reads(selection),
            sel.writes(selection),
            sel.clears(selection),
        )
    }

    fn run(session: &mut Session, events: Vec<Event>) -> Exit {
        let (tx, rx) = mpsc::channel();
        for e in events {
            tx.send(e).unwrap();
        }
        drop(tx);
        run_loop(session, &rx)
    }

    #[test]
    fn sample_has_a_finding() {
        assert_ne!(obfuscated(SAMPLE), SAMPLE);
    }

    #[test]
    fn inactive_session_never_touches_clipboard() {
        let (mut s, sel, notes) = session();
        sel.set(Clip, text(SAMPLE));
        sel.set(Primary, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Inactive);
        assert_eq!(s.on_change(Primary), Outcome::Inactive);
        s.set_active(false);
        assert!(!s.is_active());
        assert_eq!(counters(&sel, Clip), (0, 0, 0));
        assert_eq!(counters(&sel, Primary), (0, 0, 0));
        assert_eq!(sel.get(Clip), text(SAMPLE));
        assert!(notes.notices().is_empty());
    }

    #[test]
    fn active_session_replaces_text() {
        let (mut s, sel, _) = active();
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
    }

    #[test]
    fn text_without_findings_is_still_written() {
        let (mut s, sel, _) = active();
        sel.set(Clip, text("nothing to see"));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(sel.writes(Clip), 1);
        assert_eq!(sel.get(Clip), text("nothing to see"));
    }

    #[test]
    fn own_write_is_not_reprocessed() {
        let (mut s, sel, _) = active();
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(s.on_change(Clip), Outcome::Own);
        assert_eq!(sel.writes(Clip), 1);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
    }

    #[test]
    fn recopied_output_is_not_reprocessed() {
        let (mut s, sel, _) = active();
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        sel.set(Clip, text("something else"));
        sel.set(Clip, text(&obfuscated(SAMPLE)));
        assert_eq!(s.on_change(Clip), Outcome::Own);
        assert_eq!(sel.writes(Clip), 1);
    }

    #[test]
    fn files_are_left_untouched() {
        let (mut s, sel, notes) = active();
        sel.set(Clip, Content::Files);
        assert_eq!(s.on_change(Clip), Outcome::Skipped);
        assert_eq!((sel.writes(Clip), sel.clears(Clip)), (0, 0));
        assert_eq!(sel.get(Clip), Content::Files);
        assert_eq!(notes.notices(), [Notice::Started]);
    }

    #[test]
    fn non_text_is_left_untouched() {
        let (mut s, sel, notes) = active();
        sel.set(Primary, Content::Other);
        assert_eq!(s.on_change(Primary), Outcome::Skipped);
        assert_eq!((sel.writes(Primary), sel.clears(Primary)), (0, 0));
        assert_eq!(notes.notices(), [Notice::Started]);
    }

    #[test]
    fn selections_are_tracked_separately() {
        let (mut s, sel, _) = active();
        let out = obfuscated(SAMPLE);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        sel.set(Primary, text(&out));
        assert_eq!(s.on_change(Primary), Outcome::Replaced);
        assert_eq!(sel.writes(Primary), 1);
        assert_eq!(sel.get(Primary), text(&obfuscated(&out)));
        // The primary write does not disturb what counts as own on the clipboard.
        assert_eq!(s.on_change(Clip), Outcome::Own);
    }

    #[test]
    fn activation_processes_current_clipboard() {
        let (mut s, sel, notes) = session();
        sel.set(Clip, text(SAMPLE));
        s.set_active(true);
        assert!(s.is_active());
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
        assert_eq!(notes.notices(), [Notice::Started]);
        assert_eq!(counters(&sel, Primary), (0, 0, 0));
    }

    #[test]
    fn activating_twice_is_a_no_op() {
        let (mut s, sel, notes) = session();
        sel.set(Clip, text(SAMPLE));
        s.set_active(true);
        s.set_active(true);
        assert_eq!(notes.notices(), [Notice::Started]);
        assert_eq!(sel.writes(Clip), 1);
        assert_eq!(sel.reads(Clip), 1);
    }

    #[test]
    fn last_written_survives_stop_and_start() {
        let (mut s, sel, notes) = session();
        sel.set(Clip, text(SAMPLE));
        s.set_active(true);
        s.set_active(false);
        s.set_active(true);
        assert_eq!(sel.writes(Clip), 1);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
        assert_eq!(
            notes.notices(),
            [Notice::Started, Notice::Stopped, Notice::Started]
        );
    }

    #[test]
    fn write_failure_clears_and_notifies() {
        let (mut s, sel, notes) = active();
        sel.set(Clip, text(SAMPLE));
        sel.fail_write(Clip, true);
        assert_eq!(s.on_change(Clip), Outcome::Failed);
        assert_eq!(sel.clears(Clip), 1);
        assert_eq!(sel.get(Clip), Content::Other);
        assert_eq!(notes.notices(), [Notice::Started, Notice::ClipboardCleared]);
        // The clear shows up as a change too, and is skipped.
        assert_eq!(s.on_change(Clip), Outcome::Skipped);
    }

    #[test]
    fn read_failure_clears_and_notifies() {
        let (mut s, sel, notes) = active();
        sel.set(Primary, text(SAMPLE));
        sel.fail_read(Primary, true);
        assert_eq!(s.on_change(Primary), Outcome::Failed);
        assert_eq!(counters(&sel, Primary), (1, 0, 1));
        assert_eq!(sel.get(Primary), Content::Other);
        assert_eq!(notes.notices(), [Notice::Started, Notice::ClipboardCleared]);
    }

    #[test]
    fn write_and_clear_failure_notifies_original_kept() {
        let (mut s, sel, notes) = active();
        sel.set(Clip, text(SAMPLE));
        sel.fail_write(Clip, true);
        sel.fail_clear(Clip, true);
        assert_eq!(s.on_change(Clip), Outcome::Failed);
        assert_eq!(sel.get(Clip), text(SAMPLE));
        assert_eq!(
            notes.notices(),
            [Notice::Started, Notice::OriginalStillOnClipboard]
        );
    }

    #[test]
    fn failed_write_is_not_remembered() {
        let (mut s, sel, _) = active();
        let out = obfuscated(SAMPLE);
        sel.set(Clip, text(SAMPLE));
        sel.fail_write(Clip, true);
        sel.fail_clear(Clip, true);
        assert_eq!(s.on_change(Clip), Outcome::Failed);
        sel.fail_write(Clip, false);
        sel.set(Clip, text(&out));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
    }

    fn session_with(mode: PrimaryMode) -> (Session, FakeSelections, FakeNotifier) {
        let (s, sel, notes) = session();
        (s.with_primary(mode), sel, notes)
    }

    /// Like `active`, with the given PRIMARY mode.
    fn active_with(mode: PrimaryMode) -> (Session, FakeSelections, FakeNotifier) {
        let (mut s, sel, notes) = session_with(mode);
        s.set_active(true);
        assert_eq!(notes.notices(), [Notice::Started]);
        (s, sel, notes)
    }

    #[test]
    fn mirror_writes_primary_after_replacement() {
        let (mut s, sel, notes) = active_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        sel.set(Primary, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        let out = text(&obfuscated(SAMPLE));
        assert_eq!(sel.get(Clip), out);
        assert_eq!(sel.get(Primary), out);
        assert_eq!(sel.writes(Primary), 1);
        // The mirror never reads PRIMARY.
        assert_eq!(sel.reads(Primary), 0);
        assert_eq!(notes.notices(), [Notice::Started]);
    }

    #[test]
    fn off_never_touches_primary() {
        let (mut s, sel, _) = active();
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(counters(&sel, Primary), (0, 0, 0));

        let (mut s, sel, _) = active_with(PrimaryMode::Off);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(counters(&sel, Primary), (0, 0, 0));
    }

    #[test]
    fn watch_mode_does_not_mirror() {
        let (mut s, sel, notes) = active_with(PrimaryMode::Watch);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
        // The one read is the activation looking at PRIMARY, which held no text.
        assert_eq!(counters(&sel, Primary), (1, 0, 0));
        assert_eq!(notes.notices(), [Notice::Started]);
    }

    #[test]
    fn watch_replaces_primary_text() {
        let (mut s, sel, notes) = active_with(PrimaryMode::Watch);
        sel.set(Primary, text(SAMPLE));
        assert_eq!(s.on_change(Primary), Outcome::Replaced);
        assert_eq!(sel.get(Primary), text(&obfuscated(SAMPLE)));
        assert_eq!(sel.writes(Primary), 1);
        // Only PRIMARY is touched; nothing goes to the clipboard.
        assert_eq!(sel.writes(Clip), 0);
        assert_eq!(notes.notices(), [Notice::Started]);
    }

    #[test]
    fn own_primary_write_is_not_reprocessed() {
        let (mut s, sel, _) = active_with(PrimaryMode::Watch);
        sel.set(Primary, text(SAMPLE));
        assert_eq!(s.on_change(Primary), Outcome::Replaced);
        // Sagiri's own write produces a notification of its own.
        assert_eq!(s.on_change(Primary), Outcome::Own);
        assert_eq!(sel.writes(Primary), 1);
        assert_eq!(sel.get(Primary), text(&obfuscated(SAMPLE)));
    }

    #[test]
    fn watch_activation_processes_both_selections() {
        let (mut s, sel, notes) = session_with(PrimaryMode::Watch);
        sel.set(Clip, text(SAMPLE));
        sel.set(Primary, text("mail 192.0.2.7 now"));
        s.set_active(true);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
        assert_eq!(sel.get(Primary), text(&obfuscated("mail 192.0.2.7 now")));
        assert_eq!(counters(&sel, Clip), (1, 1, 0));
        assert_eq!(counters(&sel, Primary), (1, 1, 0));
        assert_eq!(notes.notices(), [Notice::Started]);

        // Other modes leave PRIMARY alone on activation.
        for mode in [PrimaryMode::Off, PrimaryMode::Mirror] {
            let (mut s, sel, _) = session_with(mode);
            sel.set(Primary, text(SAMPLE));
            s.set_active(true);
            assert_eq!(sel.reads(Primary), 0, "{mode:?}");
            assert_eq!(sel.get(Primary), text(SAMPLE), "{mode:?}");
        }
    }

    #[test]
    fn watch_failure_clears_primary_only() {
        let (mut s, sel, notes) = active_with(PrimaryMode::Watch);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        let clip = sel.get(Clip);
        let clip_counters = counters(&sel, Clip);
        sel.set(Primary, text(SAMPLE));
        sel.fail_write(Primary, true);
        assert_eq!(s.on_change(Primary), Outcome::Failed);
        assert_eq!(sel.clears(Primary), 1);
        assert_eq!(sel.get(Primary), Content::Other);
        assert_eq!(counters(&sel, Clip), clip_counters);
        assert_eq!(sel.clears(Clip), 0);
        assert_eq!(sel.get(Clip), clip);
        assert_eq!(notes.notices(), [Notice::Started, Notice::ClipboardCleared]);
    }

    #[test]
    fn mirror_skips_own_and_skipped_outcomes() {
        let (mut s, sel, _) = active_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(sel.writes(Primary), 1);
        // A repeated event for the same clipboard content.
        assert_eq!(s.on_change(Clip), Outcome::Own);
        assert_eq!(sel.writes(Primary), 1);
        // A file copy.
        sel.set(Clip, Content::Files);
        assert_eq!(s.on_change(Clip), Outcome::Skipped);
        assert_eq!(sel.writes(Primary), 1);
        assert_eq!(sel.get(Primary), text(&obfuscated(SAMPLE)));
    }

    #[test]
    fn mirror_skips_failed_and_inactive_outcomes() {
        let (mut s, sel, _) = active_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        sel.fail_write(Clip, true);
        assert_eq!(s.on_change(Clip), Outcome::Failed);
        assert_eq!(counters(&sel, Primary), (0, 0, 0));

        let (mut s, sel, _) = session_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Inactive);
        assert_eq!(counters(&sel, Primary), (0, 0, 0));
    }

    #[test]
    fn mirror_failure_keeps_clipboard_replacement() {
        let (mut s, sel, notes) = active_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        sel.set(Primary, text(SAMPLE));
        sel.fail_write(Primary, true);
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
        assert_eq!(sel.clears(Clip), 0);
        assert_eq!(sel.clears(Primary), 0);
        assert_eq!(
            notes.notices(),
            [Notice::Started, Notice::PrimaryNotUpdated]
        );
        // The clipboard write is still remembered as own.
        assert_eq!(s.on_change(Clip), Outcome::Own);
    }

    #[test]
    fn failed_mirror_is_not_remembered() {
        let (mut s, sel, _) = active_with(PrimaryMode::Mirror);
        let out = obfuscated(SAMPLE);
        sel.set(Clip, text(SAMPLE));
        sel.fail_write(Primary, true);
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        sel.fail_write(Primary, false);
        sel.set(Primary, text(&out));
        assert_eq!(s.on_change(Primary), Outcome::Replaced);
    }

    #[test]
    fn mirrored_text_counts_as_own_write() {
        let (mut s, sel, _) = active_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        assert_eq!(s.on_change(Clip), Outcome::Replaced);
        assert_eq!(s.on_change(Primary), Outcome::Own);
        assert_eq!(sel.writes(Primary), 1);
    }

    #[test]
    fn activation_mirrors_too() {
        let (mut s, sel, notes) = session_with(PrimaryMode::Mirror);
        sel.set(Clip, text(SAMPLE));
        s.set_active(true);
        let out = text(&obfuscated(SAMPLE));
        assert_eq!(sel.get(Clip), out);
        assert_eq!(sel.get(Primary), out);
        assert_eq!(notes.notices(), [Notice::Started]);
    }

    #[test]
    fn commands_reply_with_state() {
        use Command::*;
        // (command, active before, reply, exit)
        let table = [
            (Start, false, true, Exit::Shutdown),
            (Start, true, true, Exit::Shutdown),
            (Stop, false, false, Exit::Shutdown),
            (Stop, true, false, Exit::Shutdown),
            (Toggle, false, true, Exit::Shutdown),
            (Toggle, true, false, Exit::Shutdown),
            (Status, false, false, Exit::Shutdown),
            (Status, true, true, Exit::Shutdown),
            (Quit, false, false, Exit::Quit),
            (Quit, true, false, Exit::Quit),
        ];
        for (command, before, expected, exit) in table {
            let (mut s, _, _) = session();
            s.set_active(before);
            let (reply_tx, reply_rx) = mpsc::channel();
            let (tx, rx) = mpsc::channel();
            tx.send(Event::Command {
                command,
                reply: reply_tx,
            })
            .unwrap();
            if command != Quit {
                // Read the state the command left, before the loop ends.
                let (status_tx, status_rx) = mpsc::channel();
                tx.send(Event::Command {
                    command: Status,
                    reply: status_tx,
                })
                .unwrap();
                drop(tx);
                assert_eq!(run_loop(&mut s, &rx), exit, "{command:?} from {before}");
                assert_eq!(status_rx.recv(), Ok(expected), "{command:?} from {before}");
            } else {
                drop(tx);
                assert_eq!(run_loop(&mut s, &rx), exit, "{command:?} from {before}");
            }
            assert_eq!(reply_rx.recv(), Ok(expected), "{command:?} from {before}");
            assert_eq!(reply_rx.try_recv().ok(), None, "one reply per command");
        }
    }

    #[test]
    fn toggle_event_flips_state() {
        let (mut s, sel, notes) = session();
        sel.set(Clip, text(SAMPLE));
        let (status_tx, status_rx) = mpsc::channel();
        let events = vec![
            Event::Toggle,
            Event::Command {
                command: Command::Status,
                reply: status_tx.clone(),
            },
            Event::Toggle,
            Event::Command {
                command: Command::Status,
                reply: status_tx,
            },
        ];
        assert_eq!(run(&mut s, events), Exit::Shutdown);
        assert_eq!(status_rx.iter().collect::<Vec<_>>(), [true, false]);
        assert_eq!(sel.get(Clip), text(&obfuscated(SAMPLE)));
        assert_eq!(notes.notices(), [Notice::Started, Notice::Stopped]);
    }

    #[test]
    fn changed_event_runs_on_change() {
        let (mut s, sel, _) = active();
        sel.set(Primary, text(SAMPLE));
        assert_eq!(run(&mut s, vec![Event::Changed(Primary)]), Exit::Shutdown);
        assert_eq!(sel.get(Primary), text(&obfuscated(SAMPLE)));
    }

    #[test]
    fn quit_and_shutdown_stop_an_active_session() {
        for (event, exit) in [
            (
                Event::Command {
                    command: Command::Quit,
                    reply: mpsc::channel().0,
                },
                Exit::Quit,
            ),
            (Event::Shutdown, Exit::Shutdown),
        ] {
            let (mut s, _, notes) = active();
            let (tx, rx) = mpsc::channel();
            tx.send(event).unwrap();
            // Still open: the loop must end on the event itself.
            assert_eq!(run_loop(&mut s, &rx), exit);
            assert!(!s.is_active());
            assert_eq!(notes.notices(), [Notice::Started, Notice::Stopped]);
            drop(tx);
        }
    }

    #[test]
    fn events_after_the_ending_one_are_not_handled() {
        let (mut s, sel, _) = session();
        sel.set(Clip, text(SAMPLE));
        assert_eq!(
            run(&mut s, vec![Event::Shutdown, Event::Toggle]),
            Exit::Shutdown
        );
        assert!(!s.is_active());
        assert_eq!(counters(&sel, Clip), (0, 0, 0));
    }

    #[test]
    fn backend_lost_notifies_and_exits() {
        let (mut s, _, notes) = active();
        let (tx, rx) = mpsc::channel();
        tx.send(Event::BackendLost).unwrap();
        assert_eq!(run_loop(&mut s, &rx), Exit::BackendLost);
        assert!(!s.is_active());
        assert_eq!(notes.notices(), [Notice::Started, Notice::BackendLost]);
        drop(tx);
    }

    #[test]
    fn backend_lost_notifies_an_inactive_session_too() {
        let (mut s, sel, notes) = session();
        let (tx, rx) = mpsc::channel();
        tx.send(Event::BackendLost).unwrap();
        assert_eq!(run_loop(&mut s, &rx), Exit::BackendLost);
        assert!(!s.is_active());
        assert_eq!(notes.notices(), [Notice::BackendLost]);
        assert_eq!(counters(&sel, Clip), (0, 0, 0));
        drop(tx);
    }

    #[test]
    fn closed_channel_ends_the_loop() {
        let (mut s, _, notes) = session();
        assert_eq!(run(&mut s, vec![]), Exit::Shutdown);
        assert!(notes.notices().is_empty());

        let (mut s, _, notes) = active();
        assert_eq!(run(&mut s, vec![]), Exit::Shutdown);
        assert!(!s.is_active());
        assert_eq!(notes.notices(), [Notice::Started, Notice::Stopped]);
    }

    #[test]
    fn dropped_reply_receiver_is_ignored() {
        let (mut s, _, _) = session();
        let (gone_tx, gone_rx) = mpsc::channel();
        drop(gone_rx);
        let (status_tx, status_rx) = mpsc::channel();
        let events = vec![
            Event::Command {
                command: Command::Start,
                reply: gone_tx,
            },
            Event::Command {
                command: Command::Status,
                reply: status_tx,
            },
        ];
        assert_eq!(run(&mut s, events), Exit::Shutdown);
        assert_eq!(status_rx.recv(), Ok(true));
    }
}
