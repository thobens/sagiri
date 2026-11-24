//! What the user is told about a session, and the trait that tells it.

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
            Started,
            Stopped,
            ClipboardCleared,
            OriginalStillOnClipboard,
            PrimaryNotUpdated,
            BackendLost,
        ];
        let lines: std::collections::HashSet<_> =
            all.iter().map(|&n| StderrNotifier::line(n)).collect();
        assert_eq!(lines.len(), all.len());
        assert_eq!(StderrNotifier::line(Started), "session started");
        assert!(lines.iter().all(|l| !l.is_empty() && !l.contains('\n')));
    }
}
