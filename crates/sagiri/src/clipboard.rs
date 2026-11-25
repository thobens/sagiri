//! Clipboard access behind traits, so that callers can be tested without a real clipboard.
//!
//! `Clipboard` is the text-only interface the TUI uses. `SelectionClipboard` adds the
//! PRIMARY selection, tells text from files and other content, and can clear a selection.

use std::fmt;

#[cfg(target_os = "linux")]
mod targets;

/// Reads and writes the system clipboard as text.
pub trait Clipboard {
    fn get_text(&mut self) -> anyhow::Result<String>;
    fn set_text(&mut self, text: &str) -> anyhow::Result<()>;
}

/// Which selection an operation acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Selection {
    /// The regular clipboard (copy and paste).
    Clipboard,
    /// The PRIMARY selection, which a middle click pastes on X11.
    Primary,
}

/// What a selection holds.
#[derive(Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    /// A list of files, as a file manager offers it.
    Files,
    /// Anything else: an image, an empty selection, a selection without an owner.
    Other,
}

/// Shows the length of the text only, never the text itself.
impl fmt::Debug for Content {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Content::Text(s) => write!(f, "Text({} bytes)", s.len()),
            Content::Files => f.write_str("Files"),
            Content::Other => f.write_str("Other"),
        }
    }
}

/// Selection-aware clipboard access.
pub trait SelectionClipboard {
    /// What the selection holds right now.
    fn read(&mut self, selection: Selection) -> anyhow::Result<Content>;
    fn write_text(&mut self, selection: Selection, text: &str) -> anyhow::Result<()>;
    fn clear(&mut self, selection: Selection) -> anyhow::Result<()>;
}

/// The result of asking a selection for one kind of content.
///
/// No `Debug` on purpose: `Probe<String>` holds clipboard text.
pub enum Probe<T> {
    Found(T),
    /// The selection does not offer this kind of content.
    Absent,
    Failed,
}

/// Whether the owner of a selection lists a text target among the targets it offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Offer {
    /// The owner lists at least one text target.
    Text,
    /// The owner lists its targets, and none of them is text.
    NoText,
    /// Not known: the owner did not answer, or the platform has no such query.
    Unknown,
}

const TEXT_READ_FAILED: &str = "the selection could not be read as text";

/// Decides what a selection holds from a file probe, what the owner offers and a text
/// probe: files first, then text. A failed file probe counts as absent. An owner that
/// offers no text holds no text, whatever the text probe says. Otherwise a failed text
/// probe is an error; `Offer::Unknown` counts as offering text.
pub fn classify(files: Probe<usize>, offer: Offer, text: Probe<String>) -> anyhow::Result<Content> {
    match files {
        Probe::Found(n) if n > 0 => return Ok(Content::Files),
        Probe::Found(_) | Probe::Absent | Probe::Failed => {}
    }
    if offer == Offer::NoText {
        return Ok(Content::Other);
    }
    match text {
        Probe::Found(s) if !s.is_empty() => Ok(Content::Text(s)),
        Probe::Found(_) | Probe::Absent => Ok(Content::Other),
        Probe::Failed => anyhow::bail!(TEXT_READ_FAILED),
    }
}

/// `ContentNotAvailable` is `Absent`, every other error is `Failed`.
fn probe<T>(result: Result<T, arboard::Error>) -> Probe<T> {
    match result {
        Ok(v) => Probe::Found(v),
        Err(arboard::Error::ContentNotAvailable) => Probe::Absent,
        Err(_) => Probe::Failed,
    }
}

/// The real clipboard.
///
/// One `arboard::Clipboard` is created on first use and kept for the life of the app. On
/// Linux the content is served by the process that set it, so dropping it early would lose
/// the content.
#[derive(Default)]
pub struct SystemClipboard {
    inner: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    fn open(&mut self) -> anyhow::Result<&mut arboard::Clipboard> {
        if self.inner.is_none() {
            self.inner = Some(arboard::Clipboard::new()?);
        }
        match self.inner.as_mut() {
            Some(c) => Ok(c),
            None => anyhow::bail!("clipboard unavailable"),
        }
    }
}

impl SystemClipboard {
    /// Sets the selection to `text` and, on Linux, blocks until another program takes the
    /// selection over. A selection this process already owns counts as not taken over.
    /// Elsewhere it returns at once, like `write_text`.
    pub fn write_text_and_wait(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
        platform::check(selection)?;
        let cb = self.open()?;
        Ok(platform::set_text_and_wait(cb, selection, text)?)
    }
}

impl Clipboard for SystemClipboard {
    fn get_text(&mut self) -> anyhow::Result<String> {
        Ok(self.open()?.get_text()?)
    }

    fn set_text(&mut self, text: &str) -> anyhow::Result<()> {
        Ok(self.open()?.set_text(text)?)
    }
}

impl SelectionClipboard for SystemClipboard {
    fn read(&mut self, selection: Selection) -> anyhow::Result<Content> {
        platform::check(selection)?;
        let cb = self.open()?;
        let files = probe(platform::file_list(cb, selection).map(|f| f.len()));
        let offer = platform::offer(selection);
        // An owner without text is not asked for it: the answer would be its other data.
        let text = match offer {
            Offer::NoText => Probe::Absent,
            Offer::Text | Offer::Unknown => probe(platform::text(cb, selection)),
        };
        classify(files, offer, text)
    }

    fn write_text(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
        platform::check(selection)?;
        let cb = self.open()?;
        Ok(platform::set_text(cb, selection, text)?)
    }

    fn clear(&mut self, selection: Selection) -> anyhow::Result<()> {
        platform::check(selection)?;
        let cb = self.open()?;
        Ok(platform::clear(cb, selection)?)
    }
}

/// The selection-specific arboard calls. The `cfg` is the one under which arboard exports
/// its Linux extension traits.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
mod platform {
    use super::{Offer, Selection};
    use arboard::{ClearExtLinux, Clipboard, Error, GetExtLinux, LinuxClipboardKind, SetExtLinux};
    use std::path::PathBuf;

    fn kind(selection: Selection) -> LinuxClipboardKind {
        match selection {
            Selection::Clipboard => LinuxClipboardKind::Clipboard,
            Selection::Primary => LinuxClipboardKind::Primary,
        }
    }

    /// Both selections exist here.
    pub fn check(_selection: Selection) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn file_list(cb: &mut Clipboard, selection: Selection) -> Result<Vec<PathBuf>, Error> {
        cb.get().clipboard(kind(selection)).file_list()
    }

    /// Asks the owner over X11. Only on Linux, where `x11rb` is a dependency.
    #[cfg(target_os = "linux")]
    pub fn offer(selection: Selection) -> Offer {
        super::targets::offer(selection)
    }

    #[cfg(not(target_os = "linux"))]
    pub fn offer(_selection: Selection) -> Offer {
        Offer::Unknown
    }

    pub fn text(cb: &mut Clipboard, selection: Selection) -> Result<String, Error> {
        cb.get().clipboard(kind(selection)).text()
    }

    pub fn set_text(cb: &mut Clipboard, selection: Selection, text: &str) -> Result<(), Error> {
        cb.set().clipboard(kind(selection)).text(text)
    }

    pub fn set_text_and_wait(
        cb: &mut Clipboard,
        selection: Selection,
        text: &str,
    ) -> Result<(), Error> {
        cb.set().clipboard(kind(selection)).wait().text(text)
    }

    pub fn clear(cb: &mut Clipboard, selection: Selection) -> Result<(), Error> {
        cb.clear_with().clipboard(kind(selection))
    }
}

/// Without the Linux extension traits only the regular clipboard exists. `check` rejects
/// `Primary` before any of the other functions is called, so they ignore the selection.
#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
)))]
mod platform {
    use super::{Offer, Selection};
    use arboard::{Clipboard, Error};
    use std::path::PathBuf;

    pub fn check(selection: Selection) -> anyhow::Result<()> {
        match selection {
            Selection::Clipboard => Ok(()),
            Selection::Primary => {
                anyhow::bail!("the primary selection is not available on this platform")
            }
        }
    }

    pub fn file_list(cb: &mut Clipboard, _selection: Selection) -> Result<Vec<PathBuf>, Error> {
        cb.get().file_list()
    }

    /// No query for the owner's targets here.
    pub fn offer(_selection: Selection) -> Offer {
        Offer::Unknown
    }

    pub fn text(cb: &mut Clipboard, _selection: Selection) -> Result<String, Error> {
        cb.get().text()
    }

    pub fn set_text(cb: &mut Clipboard, _selection: Selection, text: &str) -> Result<(), Error> {
        cb.set().text(text)
    }

    pub fn set_text_and_wait(
        cb: &mut Clipboard,
        _selection: Selection,
        text: &str,
    ) -> Result<(), Error> {
        cb.set().text(text)
    }

    pub fn clear(cb: &mut Clipboard, _selection: Selection) -> Result<(), Error> {
        cb.clear()
    }
}

#[cfg(test)]
pub use fake::{FakeClipboard, FakeSelections};

#[cfg(test)]
mod fake {
    use super::{Clipboard, Content, Selection, SelectionClipboard};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct State {
        content: String,
        fail_get: bool,
        fail_set: bool,
    }

    /// A clipboard in memory. Clones share the same state, so a test can keep a handle
    /// after giving one to the app.
    #[derive(Clone, Default)]
    pub struct FakeClipboard(Rc<RefCell<State>>);

    impl FakeClipboard {
        pub fn with_content(text: &str) -> Self {
            let c = Self::default();
            c.0.borrow_mut().content = text.to_string();
            c
        }
        pub fn content(&self) -> String {
            self.0.borrow().content.clone()
        }
        pub fn fail_get(&self, fail: bool) {
            self.0.borrow_mut().fail_get = fail;
        }
        pub fn fail_set(&self, fail: bool) {
            self.0.borrow_mut().fail_set = fail;
        }
    }

    impl Clipboard for FakeClipboard {
        fn get_text(&mut self) -> anyhow::Result<String> {
            let s = self.0.borrow();
            if s.fail_get {
                anyhow::bail!("fake read failure");
            }
            Ok(s.content.clone())
        }
        fn set_text(&mut self, text: &str) -> anyhow::Result<()> {
            let mut s = self.0.borrow_mut();
            if s.fail_set {
                anyhow::bail!("fake write failure");
            }
            s.content = text.to_string();
            Ok(())
        }
    }

    /// The state of one selection in `FakeSelections`.
    struct Slot {
        content: Content,
        fail_read: bool,
        fail_write: bool,
        fail_clear: bool,
        reads: usize,
        writes: usize,
        clears: usize,
    }

    impl Default for Slot {
        fn default() -> Self {
            Self {
                content: Content::Other,
                fail_read: false,
                fail_write: false,
                fail_clear: false,
                reads: 0,
                writes: 0,
                clears: 0,
            }
        }
    }

    #[derive(Default)]
    struct Slots {
        clipboard: Slot,
        primary: Slot,
    }

    impl Slots {
        fn slot(&mut self, selection: Selection) -> &mut Slot {
            match selection {
                Selection::Clipboard => &mut self.clipboard,
                Selection::Primary => &mut self.primary,
            }
        }
    }

    /// Both selections in memory, each starting as `Content::Other`. Clones share the same
    /// state. Every call to `read`, `write_text` or `clear` is counted, failed ones too.
    #[derive(Clone, Default)]
    pub struct FakeSelections(Rc<RefCell<Slots>>);

    impl FakeSelections {
        fn with<R>(&self, selection: Selection, f: impl FnOnce(&mut Slot) -> R) -> R {
            f(self.0.borrow_mut().slot(selection))
        }
        pub fn set(&self, selection: Selection, content: Content) {
            self.with(selection, |s| s.content = content);
        }
        pub fn get(&self, selection: Selection) -> Content {
            self.with(selection, |s| s.content.clone())
        }
        pub fn fail_read(&self, selection: Selection, fail: bool) {
            self.with(selection, |s| s.fail_read = fail);
        }
        pub fn fail_write(&self, selection: Selection, fail: bool) {
            self.with(selection, |s| s.fail_write = fail);
        }
        pub fn fail_clear(&self, selection: Selection, fail: bool) {
            self.with(selection, |s| s.fail_clear = fail);
        }
        pub fn reads(&self, selection: Selection) -> usize {
            self.with(selection, |s| s.reads)
        }
        pub fn writes(&self, selection: Selection) -> usize {
            self.with(selection, |s| s.writes)
        }
        pub fn clears(&self, selection: Selection) -> usize {
            self.with(selection, |s| s.clears)
        }
    }

    impl SelectionClipboard for FakeSelections {
        fn read(&mut self, selection: Selection) -> anyhow::Result<Content> {
            self.with(selection, |s| {
                s.reads += 1;
                if s.fail_read {
                    anyhow::bail!("fake read failure");
                }
                Ok(s.content.clone())
            })
        }
        fn write_text(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
            self.with(selection, |s| {
                s.writes += 1;
                if s.fail_write {
                    anyhow::bail!("fake write failure");
                }
                s.content = Content::Text(text.to_string());
                Ok(())
            })
        }
        fn clear(&mut self, selection: Selection) -> anyhow::Result<()> {
            self.with(selection, |s| {
                s.clears += 1;
                if s.fail_clear {
                    anyhow::bail!("fake clear failure");
                }
                s.content = Content::Other;
                Ok(())
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Selection::{Clipboard as Clip, Primary};

    fn text(s: &str) -> Probe<String> {
        Probe::Found(s.to_string())
    }

    /// Every kind of text probe.
    fn all_text_probes() -> [Probe<String>; 4] {
        [
            text("host 192.0.2.7"),
            text(""),
            Probe::Absent,
            Probe::Failed,
        ]
    }

    /// Every file probe that does not find files.
    fn no_files() -> [Probe<usize>; 3] {
        [Probe::Absent, Probe::Found(0), Probe::Failed]
    }

    /// The offers under which the text probe decides.
    const TEXT_OFFERS: [Offer; 2] = [Offer::Text, Offer::Unknown];

    #[test]
    fn classify_files_win_over_text() {
        for offer in [Offer::Text, Offer::NoText, Offer::Unknown] {
            let cases = [
                text("/home/alice/report.txt"),
                text(""),
                Probe::Absent,
                Probe::Failed,
            ];
            for t in cases {
                assert_eq!(classify(Probe::Found(2), offer, t).unwrap(), Content::Files);
            }
            assert_eq!(
                classify(Probe::Found(1), offer, Probe::Failed).unwrap(),
                Content::Files
            );
            assert_eq!(
                classify(Probe::Found(1), offer, Probe::Absent).unwrap(),
                Content::Files
            );
        }
    }

    #[test]
    fn classify_text() {
        for offer in TEXT_OFFERS {
            for files in [Probe::Absent, Probe::Found(0)] {
                let c = classify(files, offer, text("host 192.0.2.7")).unwrap();
                assert_eq!(c, Content::Text("host 192.0.2.7".into()));
            }
        }
    }

    #[test]
    fn classify_no_text_is_other() {
        for offer in TEXT_OFFERS {
            for files in no_files() {
                assert_eq!(
                    classify(files, offer, Probe::Absent).unwrap(),
                    Content::Other
                );
            }
        }
    }

    #[test]
    fn classify_empty_text_is_other() {
        for offer in TEXT_OFFERS {
            for files in no_files() {
                assert_eq!(classify(files, offer, text("")).unwrap(), Content::Other);
            }
        }
    }

    /// Fail closed: an owner that offers text, or might, and whose read fails is an error.
    #[test]
    fn classify_failed_text_is_error() {
        for offer in TEXT_OFFERS {
            for files in no_files() {
                let err = classify(files, offer, Probe::Failed).unwrap_err();
                assert_eq!(err.to_string(), "the selection could not be read as text");
            }
        }
    }

    #[test]
    fn classify_failed_files_falls_through_to_text() {
        for offer in TEXT_OFFERS {
            let cases = [
                (text("x"), Content::Text("x".into())),
                (text(""), Content::Other),
                (Probe::Absent, Content::Other),
            ];
            for (t, expected) in cases {
                assert_eq!(classify(Probe::Failed, offer, t).unwrap(), expected);
            }
            assert!(classify(Probe::Failed, offer, Probe::Failed).is_err());
        }
    }

    /// An owner such as `xclip -t image/png` answers a text request with its image, so the
    /// text read fails; what the owner offers decides.
    #[test]
    fn classify_owner_without_text_is_other_whatever_the_text_read_returns() {
        for t in all_text_probes() {
            assert_eq!(
                classify(Probe::Absent, Offer::NoText, t).unwrap(),
                Content::Other
            );
        }
        for t in all_text_probes() {
            assert_eq!(
                classify(Probe::Found(0), Offer::NoText, t).unwrap(),
                Content::Other
            );
        }
        for t in all_text_probes() {
            assert_eq!(
                classify(Probe::Failed, Offer::NoText, t).unwrap(),
                Content::Other
            );
        }
    }

    #[test]
    fn content_debug_hides_text() {
        let shown = format!("{:?}", Content::Text("secret".into()));
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains('6'), "{shown}");
        assert_eq!(format!("{:?}", Content::Files), "Files");
        assert_eq!(format!("{:?}", Content::Other), "Other");
    }

    #[test]
    fn fake_selections_are_independent() {
        let fake = FakeSelections::default();
        assert_eq!(fake.get(Clip), Content::Other);
        assert_eq!(fake.get(Primary), Content::Other);

        let mut handle = fake.clone();
        handle.write_text(Clip, "a").unwrap();
        assert_eq!(fake.get(Clip), Content::Text("a".into()));
        assert_eq!(fake.get(Primary), Content::Other);

        fake.set(Primary, Content::Files);
        assert_eq!(handle.read(Primary).unwrap(), Content::Files);
        assert_eq!(handle.read(Clip).unwrap(), Content::Text("a".into()));

        handle.clear(Clip).unwrap();
        assert_eq!(fake.get(Clip), Content::Other);
        assert_eq!(fake.get(Primary), Content::Files);
    }

    #[test]
    fn fake_counts_reads_writes_and_clears() {
        let fake = FakeSelections::default();
        let mut handle = fake.clone();
        handle.read(Clip).unwrap();
        handle.read(Clip).unwrap();
        handle.write_text(Primary, "b").unwrap();
        handle.clear(Primary).unwrap();
        handle.clear(Primary).unwrap();
        handle.clear(Clip).unwrap();

        assert_eq!((fake.reads(Clip), fake.reads(Primary)), (2, 0));
        assert_eq!((fake.writes(Clip), fake.writes(Primary)), (0, 1));
        assert_eq!((fake.clears(Clip), fake.clears(Primary)), (1, 2));
    }

    /// Runs all three operations on both selections and reports which ones succeeded,
    /// in the order (read, write, clear) for Clipboard, then for Primary.
    fn outcomes(fake: &FakeSelections) -> [bool; 6] {
        let mut h = fake.clone();
        [
            h.read(Clip).is_ok(),
            h.write_text(Clip, "c").is_ok(),
            h.clear(Clip).is_ok(),
            h.read(Primary).is_ok(),
            h.write_text(Primary, "c").is_ok(),
            h.clear(Primary).is_ok(),
        ]
    }

    #[test]
    fn fake_failure_switches() {
        type Switch = fn(&FakeSelections, Selection, bool);
        let switches: [(Switch, usize); 3] = [
            (FakeSelections::fail_read, 0),
            (FakeSelections::fail_write, 1),
            (FakeSelections::fail_clear, 2),
        ];
        for (switch, op) in switches {
            for (selection, offset) in [(Clip, 0), (Primary, 3)] {
                let fake = FakeSelections::default();
                switch(&fake, selection, true);
                let mut expected = [true; 6];
                expected[op + offset] = false;
                assert_eq!(outcomes(&fake), expected, "op {op} on {selection:?}");

                switch(&fake, selection, false);
                assert_eq!(outcomes(&fake), [true; 6]);
            }
        }
    }

    #[test]
    fn fake_failed_write_and_clear_keep_content() {
        let fake = FakeSelections::default();
        fake.set(Clip, Content::Text("keep".into()));
        fake.fail_write(Clip, true);
        fake.fail_clear(Clip, true);
        let mut h = fake.clone();
        assert!(h.write_text(Clip, "new").is_err());
        assert!(h.clear(Clip).is_err());
        assert_eq!(fake.get(Clip), Content::Text("keep".into()));
    }
}
