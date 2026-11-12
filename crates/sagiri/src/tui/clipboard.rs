//! Clipboard access behind a trait, so that the app can be tested without a real clipboard.

/// Reads and writes the system clipboard as text.
pub trait Clipboard {
    fn get_text(&mut self) -> anyhow::Result<String>;
    fn set_text(&mut self, text: &str) -> anyhow::Result<()>;
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

impl Clipboard for SystemClipboard {
    fn get_text(&mut self) -> anyhow::Result<String> {
        Ok(self.open()?.get_text()?)
    }

    fn set_text(&mut self, text: &str) -> anyhow::Result<()> {
        Ok(self.open()?.set_text(text)?)
    }
}

#[cfg(test)]
pub use fake::FakeClipboard;

#[cfg(test)]
mod fake {
    use super::Clipboard;
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
}
