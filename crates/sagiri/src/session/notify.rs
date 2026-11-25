//! What the user is told about a session, and the trait that tells it.

use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::thread;

use sagiri_core::config::SessionConfig;

/// What the user is told. No variant carries text, so no content can leak.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    Started,
    Stopped,
    /// Replacing failed; the selection was cleared.
    ClipboardCleared,
    /// Replacing and clearing both failed.
    OriginalStillOnClipboard,
    /// The mirror write to the primary selection failed.
    PrimaryNotUpdated,
    /// The display connection ended.
    BackendLost,
}

/// Delivers notices to the user.
pub trait Notifier {
    fn notify(&mut self, notice: Notice);
}

/// Writes one fixed line per notice to stderr.
pub struct StderrNotifier;

impl StderrNotifier {
    /// The line for `notice`, without the program prefix.
    fn line(notice: Notice) -> &'static str {
        match notice {
            Notice::Started => "session started",
            Notice::Stopped => "session stopped",
            Notice::ClipboardCleared => "the clipboard could not be replaced and was cleared",
            Notice::OriginalStillOnClipboard => {
                "the clipboard could not be replaced or cleared; the original is still on it"
            }
            Notice::PrimaryNotUpdated => "the primary selection could not be updated",
            Notice::BackendLost => "the display connection was lost; the session ended",
        }
    }
}

impl Notifier for StderrNotifier {
    fn notify(&mut self, notice: Notice) {
        eprintln!("sagiri-session: {}", Self::line(notice));
    }
}

/// Shows each notice as a desktop notification by running `notify-send`.
///
/// The arguments are string literals only, so no clipboard text can reach them.
pub struct CommandNotifier {
    program: OsString,
    warnings: usize,
}

impl CommandNotifier {
    /// A notifier that runs `notify-send`.
    pub fn new() -> Self {
        Self::with_program("notify-send")
    }

    /// A notifier that runs `program` with the `notify-send` arguments.
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            warnings: 0,
        }
    }

    /// The `notify-send` arguments for `notice`: app name, urgency, summary, body.
    pub fn argv(notice: Notice) -> Vec<&'static str> {
        let (urgency, summary, body) = match notice {
            Notice::Started => (
                "--urgency=normal",
                "Sagiri session started",
                "Copied text is now obfuscated.",
            ),
            Notice::Stopped => (
                "--urgency=normal",
                "Sagiri session stopped",
                "Copied text is no longer changed.",
            ),
            Notice::ClipboardCleared => (
                "--urgency=critical",
                "Sagiri could not replace the clipboard",
                "The clipboard was cleared instead.",
            ),
            Notice::OriginalStillOnClipboard => (
                "--urgency=critical",
                "Sagiri could not replace the clipboard",
                "The clipboard still holds the original text.",
            ),
            Notice::PrimaryNotUpdated => (
                "--urgency=normal",
                "Sagiri could not update the primary selection",
                "A middle click may paste other text than the clipboard holds.",
            ),
            Notice::BackendLost => (
                "--urgency=critical",
                "Sagiri session ended",
                "The connection to the display was lost.",
            ),
        };
        vec!["--app-name=Sagiri", urgency, summary, body]
    }

    /// How many warnings were written to stderr. At most one.
    #[cfg(test)]
    fn warnings(&self) -> usize {
        self.warnings
    }
}

impl Default for CommandNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Notifier for CommandNotifier {
    fn notify(&mut self, notice: Notice) {
        let spawned = Command::new(&self.program)
            .args(Self::argv(notice))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(mut child) => {
                // Reap the child off the session thread. If no thread can be started, the
                // child stays unreaped rather than delaying the session.
                let _ = thread::Builder::new()
                    .name("notify-send".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
            }
            Err(_) => {
                if self.warnings == 0 {
                    self.warnings += 1;
                    eprintln!(
                        "sagiri-session: notify-send is not available; desktop notifications are off"
                    );
                }
            }
        }
    }
}

/// Forwards each notice to every notifier it holds, in order.
pub struct Notifiers(pub Vec<Box<dyn Notifier>>);

impl Notifiers {
    /// The notifiers of `sagiri-session run`: stderr always, desktop notifications as well
    /// when `config.notify` is set.
    pub fn for_session(config: &SessionConfig) -> Self {
        let mut all: Vec<Box<dyn Notifier>> = vec![Box::new(StderrNotifier)];
        if config.notify {
            all.push(Box::new(CommandNotifier::new()));
        }
        Notifiers(all)
    }
}

impl Notifier for Notifiers {
    fn notify(&mut self, notice: Notice) {
        for notifier in &mut self.0 {
            notifier.notify(notice);
        }
    }
}

#[cfg(test)]
pub use fake::FakeNotifier;

#[cfg(test)]
mod fake {
    use super::{Notice, Notifier};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records every notice it receives. Clones share the same record, so a test can keep
    /// a handle after giving one to the session.
    #[derive(Clone, Default)]
    pub struct FakeNotifier(Rc<RefCell<Vec<Notice>>>);

    impl FakeNotifier {
        /// The notices received so far, oldest first.
        pub fn notices(&self) -> Vec<Notice> {
            self.0.borrow().clone()
        }
    }

    impl Notifier for FakeNotifier {
        fn notify(&mut self, notice: Notice) {
            self.0.borrow_mut().push(notice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_notifier_clones_share_record() {
        let fake = FakeNotifier::default();
        let mut handle = fake.clone();
        handle.notify(Notice::Started);
        handle.notify(Notice::Stopped);
        assert_eq!(fake.notices(), [Notice::Started, Notice::Stopped]);
    }

    #[test]
    fn stderr_lines_are_fixed_and_distinct() {
        use Notice::*;
        let all = [
            (Started, "session started"),
            (Stopped, "session stopped"),
            (
                ClipboardCleared,
                "the clipboard could not be replaced and was cleared",
            ),
            (
                OriginalStillOnClipboard,
                "the clipboard could not be replaced or cleared; the original is still on it",
            ),
            (
                PrimaryNotUpdated,
                "the primary selection could not be updated",
            ),
            (
                BackendLost,
                "the display connection was lost; the session ended",
            ),
        ];
        for (notice, line) in all {
            assert_eq!(StderrNotifier::line(notice), line, "{notice:?}");
        }
        let lines: std::collections::HashSet<_> = all.iter().map(|(_, l)| l).collect();
        assert_eq!(lines.len(), all.len());
    }

    const ALL: [Notice; 6] = [
        Notice::Started,
        Notice::Stopped,
        Notice::ClipboardCleared,
        Notice::OriginalStillOnClipboard,
        Notice::PrimaryNotUpdated,
        Notice::BackendLost,
    ];

    #[test]
    fn argv_for_each_notice() {
        use Notice::*;
        let table = [
            (
                Started,
                [
                    "--app-name=Sagiri",
                    "--urgency=normal",
                    "Sagiri session started",
                    "Copied text is now obfuscated.",
                ],
            ),
            (
                Stopped,
                [
                    "--app-name=Sagiri",
                    "--urgency=normal",
                    "Sagiri session stopped",
                    "Copied text is no longer changed.",
                ],
            ),
            (
                ClipboardCleared,
                [
                    "--app-name=Sagiri",
                    "--urgency=critical",
                    "Sagiri could not replace the clipboard",
                    "The clipboard was cleared instead.",
                ],
            ),
            (
                OriginalStillOnClipboard,
                [
                    "--app-name=Sagiri",
                    "--urgency=critical",
                    "Sagiri could not replace the clipboard",
                    "The clipboard still holds the original text.",
                ],
            ),
            (
                PrimaryNotUpdated,
                [
                    "--app-name=Sagiri",
                    "--urgency=normal",
                    "Sagiri could not update the primary selection",
                    "A middle click may paste other text than the clipboard holds.",
                ],
            ),
            (
                BackendLost,
                [
                    "--app-name=Sagiri",
                    "--urgency=critical",
                    "Sagiri session ended",
                    "The connection to the display was lost.",
                ],
            ),
        ];
        assert_eq!(table.len(), ALL.len());
        for (notice, expected) in table {
            assert_eq!(CommandNotifier::argv(notice), expected, "{notice:?}");
        }
    }

    #[test]
    fn failures_use_critical_urgency() {
        let critical: Vec<Notice> = ALL
            .into_iter()
            .filter(|&n| CommandNotifier::argv(n).contains(&"--urgency=critical"))
            .collect();
        assert_eq!(
            critical,
            [
                Notice::ClipboardCleared,
                Notice::OriginalStillOnClipboard,
                Notice::BackendLost
            ]
        );
        for notice in ALL {
            if !critical.contains(&notice) {
                assert!(CommandNotifier::argv(notice).contains(&"--urgency=normal"));
            }
        }
    }

    #[test]
    fn argv_is_static() {
        // Compiles only while `argv` returns string literals: no runtime text, such as
        // clipboard content, can be passed to the notification program.
        let argv: fn(Notice) -> Vec<&'static str> = CommandNotifier::argv;
        for notice in ALL {
            assert_eq!(argv(notice).len(), 4);
        }
    }

    #[test]
    fn missing_program_warns_once_and_continues() {
        let mut notifier = CommandNotifier::with_program("sagiri-test-no-such-program");
        notifier.notify(Notice::Started);
        notifier.notify(Notice::Stopped);
        assert_eq!(notifier.warnings(), 1);
    }

    #[test]
    fn notifier_does_not_block() {
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let script = dir.path().join("slow-notify");
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.tmp'\nmv '{0}.tmp' '{0}'\nsleep 2\n",
            args.display()
        );
        // A child process writes the script. Had this process written it, a child forked by
        // a concurrent test could inherit the open file, and running the script would fail
        // with ETXTBSY ("text file busy").
        let status = Command::new("sh")
            .arg("-c")
            .arg("printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"")
            .arg("sh")
            .arg(&body)
            .arg(&script)
            .status()
            .unwrap();
        assert!(status.success());

        let mut notifier = CommandNotifier::with_program(&script);
        let start = Instant::now();
        notifier.notify(Notice::Started);
        assert!(start.elapsed() < Duration::from_millis(200));
        assert_eq!(notifier.warnings(), 0);

        // The script did run, with the fixed arguments.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !args.exists() {
            assert!(Instant::now() < deadline, "the script did not run");
            thread::sleep(Duration::from_millis(20));
        }
        let expected: String = CommandNotifier::argv(Notice::Started)
            .iter()
            .map(|a| format!("{a}\n"))
            .collect();
        assert_eq!(std::fs::read_to_string(&args).unwrap(), expected);
    }

    #[test]
    fn notifiers_forward_to_all() {
        let first = FakeNotifier::default();
        let second = FakeNotifier::default();
        let mut all = Notifiers(vec![Box::new(first.clone()), Box::new(second.clone())]);
        all.notify(Notice::BackendLost);
        assert_eq!(first.notices(), [Notice::BackendLost]);
        assert_eq!(second.notices(), [Notice::BackendLost]);
    }

    #[test]
    fn notify_false_uses_stderr_only() {
        let off = SessionConfig {
            notify: false,
            ..SessionConfig::default()
        };
        assert_eq!(Notifiers::for_session(&off).0.len(), 1);
        let on = SessionConfig::default();
        assert!(on.notify);
        assert_eq!(Notifiers::for_session(&on).0.len(), 2);
    }
}
