//! Application state. No terminal or clipboard code lives here.

use std::ops::Range;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sagiri_core::engine::{Engine, render};

use super::clipboard::Clipboard;
use super::ui::line_width;

/// Lines moved by a page scroll when the viewport size is not known yet.
const DEFAULT_PAGE: usize = 10;
/// Columns moved by one press of a horizontal scroll key.
const H_STEP: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Replace the text (terminal paste).
    Paste(String),
    /// Replace the text with the system clipboard.
    PasteClipboard,
    /// Copy the obfuscated text to the system clipboard.
    Copy,
    Clear,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
    ScrollLeft,
    ScrollRight,
    Quit,
}

/// One text split into lines, for cheap access to the visible ones.
#[derive(Default)]
struct Lines {
    text: String,
    ranges: Vec<Range<usize>>,
    max_width: usize,
}

impl Lines {
    fn new(text: String) -> Self {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                ranges.push(start..i);
                start = i + 1;
            }
        }
        if start < text.len() {
            ranges.push(start..text.len());
        }
        let max_width = ranges
            .iter()
            .map(|r| line_width(&text[r.clone()]))
            .max()
            .unwrap_or(0);
        Self {
            text,
            ranges,
            max_width,
        }
    }

    fn get(&self, index: usize) -> &str {
        self.ranges.get(index).map_or("", |r| &self.text[r.clone()])
    }
}

pub struct App {
    engine: Engine,
    clipboard: Box<dyn Clipboard>,
    original: Lines,
    output: Lines,
    findings: usize,
    v_offset: usize,
    h_offset: usize,
    page: usize,
    message: Option<String>,
    quit: bool,
}

impl App {
    pub fn new(engine: Engine, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            engine,
            clipboard,
            original: Lines::default(),
            output: Lines::default(),
            findings: 0,
            v_offset: 0,
            h_offset: 0,
            page: DEFAULT_PAGE,
            message: None,
            quit: false,
        }
    }

    /// Tells the app how many lines fit on screen, for page scrolling.
    pub fn set_page_size(&mut self, lines: usize) {
        self.page = lines.max(1);
    }

    pub fn update(&mut self, action: Action) {
        self.message = None;
        match action {
            Action::Paste(text) => self.set_text(&text),
            Action::PasteClipboard => match self.clipboard.get_text() {
                Ok(text) => self.set_text(&text),
                Err(e) => self.message = Some(format!("Cannot read the clipboard: {e}")),
            },
            Action::Copy => self.copy(),
            Action::Clear => self.set_text(""),
            Action::ScrollUp => self.scroll_v(-1),
            Action::ScrollDown => self.scroll_v(1),
            Action::PageUp => self.scroll_v(-(self.page as isize)),
            Action::PageDown => self.scroll_v(self.page as isize),
            Action::Top => self.v_offset = 0,
            Action::Bottom => self.v_offset = self.max_v(),
            Action::ScrollLeft => self.h_offset = self.h_offset.saturating_sub(H_STEP),
            Action::ScrollRight => self.h_offset = (self.h_offset + H_STEP).min(self.max_h()),
            Action::Quit => self.quit = true,
        }
    }

    fn set_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let findings = self.engine.analyze(&text);
        let rendered = render(&text, &findings, |_| true);
        self.findings = findings.len();
        self.original = Lines::new(text);
        self.output = Lines::new(rendered.text);
        self.v_offset = 0;
        self.h_offset = 0;
    }

    fn copy(&mut self) {
        let text = &self.output.text;
        if text.is_empty() {
            self.message = Some("Nothing to copy".to_string());
            return;
        }
        let n = text.chars().count();
        self.message = Some(match self.clipboard.set_text(text) {
            Ok(()) => format!("Copied {n} characters"),
            Err(e) => format!("Cannot write to the clipboard: {e}"),
        });
    }

    fn max_v(&self) -> usize {
        self.original
            .ranges
            .len()
            .max(self.output.ranges.len())
            .saturating_sub(1)
    }

    fn max_h(&self) -> usize {
        self.original
            .max_width
            .max(self.output.max_width)
            .saturating_sub(1)
    }

    fn scroll_v(&mut self, delta: isize) {
        self.v_offset = self.v_offset.saturating_add_signed(delta).min(self.max_v());
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn is_empty(&self) -> bool {
        self.original.text.is_empty()
    }

    #[cfg(test)]
    pub fn original(&self) -> &str {
        &self.original.text
    }

    #[cfg(test)]
    pub fn output(&self) -> &str {
        &self.output.text
    }

    pub fn original_line(&self, index: usize) -> &str {
        self.original.get(index)
    }

    pub fn output_line(&self, index: usize) -> &str {
        self.output.get(index)
    }

    pub fn line_count(&self) -> usize {
        self.original.ranges.len()
    }

    pub fn finding_count(&self) -> usize {
        self.findings
    }

    pub fn v_offset(&self) -> usize {
        self.v_offset
    }

    pub fn h_offset(&self) -> usize {
        self.h_offset
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

/// Maps a terminal event to an action. Unbound input gives `None`.
pub fn action_for(event: &Event) -> Option<Action> {
    match event {
        Event::Paste(text) => Some(Action::Paste(text.clone())),
        Event::Key(key) if key.kind != KeyEventKind::Release => key_action(key),
        _ => None,
    }
}

fn key_action(key: &KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('v') => Some(Action::PasteClipboard),
            KeyCode::Char('l') => Some(Action::Clear),
            KeyCode::Char('c') => Some(Action::Quit),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char('p') => Some(Action::PasteClipboard),
        KeyCode::Char('y') | KeyCode::Enter => Some(Action::Copy),
        KeyCode::Char('x') => Some(Action::Clear),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::ScrollUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::ScrollDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Char('g') => Some(Action::Top),
        KeyCode::Char('G') => Some(Action::Bottom),
        KeyCode::Left | KeyCode::Char('h') => Some(Action::ScrollLeft),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::ScrollRight),
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::tui::clipboard::FakeClipboard;
    use sagiri_core::key::Key;
    use sagiri_core::pseudo::Pseudonymizer;
    use sagiri_core::rules::default_rules;

    pub fn test_engine() -> Engine {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        Engine::new(Pseudonymizer::new(&Key::from_bytes(bytes)), default_rules())
    }

    pub fn app() -> (App, FakeClipboard) {
        let clip = FakeClipboard::default();
        (App::new(test_engine(), Box::new(clip.clone())), clip)
    }

    fn paste(app: &mut App, text: &str) {
        app.update(Action::Paste(text.to_string()));
    }

    #[test]
    fn paste_obfuscates() {
        let (mut app, _) = app();
        let text = "server 172.55.0.1 is down";
        paste(&mut app, text);
        assert_eq!(app.output(), test_engine().obfuscate(text));
        assert!(!app.output().contains("172.55.0.1"));
        assert_eq!(app.original(), text);
        assert_eq!(app.finding_count(), 1);
    }

    #[test]
    fn paste_replaces_previous_text() {
        let (mut app, _) = app();
        paste(&mut app, "first 172.55.0.1\nsecond line");
        app.update(Action::ScrollDown);
        paste(&mut app, "other");
        assert_eq!(app.original(), "other");
        assert_eq!(app.output(), "other");
        assert_eq!(app.line_count(), 1);
        assert_eq!(app.finding_count(), 0);
        assert_eq!(app.v_offset(), 0);
    }

    #[test]
    fn same_value_in_two_pastes_gets_same_replacement() {
        let (mut app, _) = app();
        paste(&mut app, "a 172.55.0.1 b");
        let first = app.output().to_string();
        paste(&mut app, "x");
        paste(&mut app, "a 172.55.0.1 b");
        assert_eq!(app.output(), first);
    }

    #[test]
    fn line_breaks_are_normalized() {
        let (mut app, _) = app();
        paste(&mut app, "a\r\nb\rc");
        assert_eq!(app.line_count(), 3);
        assert!(!app.original().contains('\r'));
        assert!(!app.output().contains('\r'));
    }

    #[test]
    fn copy_sets_clipboard_to_output() {
        let (mut app, clip) = app();
        paste(&mut app, "server 172.55.0.1 is down\n");
        app.update(Action::Copy);
        assert_eq!(clip.content(), app.output());
        let n = app.output().chars().count();
        assert_eq!(
            app.message(),
            Some(format!("Copied {n} characters").as_str())
        );
    }

    #[test]
    fn ctrl_v_reads_clipboard() {
        let clip = FakeClipboard::with_content("host 172.55.0.1\r\nx");
        let mut app = App::new(test_engine(), Box::new(clip));
        app.update(action_for(&ctrl('v')).unwrap());
        assert_eq!(app.original(), "host 172.55.0.1\nx");
        assert!(!app.output().contains("172.55.0.1"));
    }

    #[test]
    fn clipboard_failure_becomes_status_message() {
        let (mut app, clip) = app();
        clip.fail_get(true);
        app.update(Action::PasteClipboard);
        assert!(app.message().unwrap().contains("Cannot read"));
        assert!(!app.should_quit());

        paste(&mut app, "172.55.0.1");
        clip.fail_set(true);
        app.update(Action::Copy);
        assert!(app.message().unwrap().contains("Cannot write"));
        assert!(!app.should_quit());
        assert_eq!(app.original(), "172.55.0.1");
    }

    #[test]
    fn clear_resets_text_and_scroll() {
        let (mut app, _) = app();
        paste(&mut app, "a\nb\nc\n");
        app.update(Action::ScrollDown);
        app.update(Action::ScrollRight);
        app.update(Action::Clear);
        assert!(app.is_empty());
        assert_eq!(app.output(), "");
        assert_eq!((app.v_offset(), app.h_offset()), (0, 0));
        assert_eq!(app.finding_count(), 0);
    }

    #[test]
    fn scroll_is_clamped() {
        let (mut app, _) = app();
        paste(&mut app, "abcdefghij\nb\nc");
        app.update(Action::ScrollUp);
        app.update(Action::PageUp);
        app.update(Action::ScrollLeft);
        assert_eq!((app.v_offset(), app.h_offset()), (0, 0));
        for _ in 0..10 {
            app.update(Action::ScrollDown);
            app.update(Action::ScrollRight);
        }
        assert_eq!((app.v_offset(), app.h_offset()), (2, 9));
        app.update(Action::PageDown);
        assert_eq!(app.v_offset(), 2);
        app.update(Action::Top);
        assert_eq!(app.v_offset(), 0);
        app.update(Action::Bottom);
        assert_eq!(app.v_offset(), 2);
        for _ in 0..10 {
            app.update(Action::ScrollLeft);
        }
        assert_eq!(app.h_offset(), 0);
    }

    #[test]
    fn scroll_on_empty_text_stays_zero() {
        let (mut app, _) = app();
        for a in [Action::Bottom, Action::PageDown, Action::ScrollRight] {
            app.update(a);
        }
        assert_eq!((app.v_offset(), app.h_offset()), (0, 0));
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    #[test]
    fn key_mapping() {
        let ch = |c| key(KeyCode::Char(c));
        let table = [
            (Event::Paste("x".into()), Some(Action::Paste("x".into()))),
            (ctrl('v'), Some(Action::PasteClipboard)),
            (ch('p'), Some(Action::PasteClipboard)),
            (ch('y'), Some(Action::Copy)),
            (key(KeyCode::Enter), Some(Action::Copy)),
            (ch('x'), Some(Action::Clear)),
            (ctrl('l'), Some(Action::Clear)),
            (key(KeyCode::Up), Some(Action::ScrollUp)),
            (ch('k'), Some(Action::ScrollUp)),
            (key(KeyCode::Down), Some(Action::ScrollDown)),
            (ch('j'), Some(Action::ScrollDown)),
            (key(KeyCode::PageUp), Some(Action::PageUp)),
            (key(KeyCode::PageDown), Some(Action::PageDown)),
            (ch('g'), Some(Action::Top)),
            (ch('G'), Some(Action::Bottom)),
            (key(KeyCode::Left), Some(Action::ScrollLeft)),
            (ch('h'), Some(Action::ScrollLeft)),
            (key(KeyCode::Right), Some(Action::ScrollRight)),
            (ch('l'), Some(Action::ScrollRight)),
            (ch('q'), Some(Action::Quit)),
            (key(KeyCode::Esc), Some(Action::Quit)),
            (ctrl('c'), Some(Action::Quit)),
            (ch('z'), None),
            (ctrl('z'), None),
            (key(KeyCode::F(1)), None),
            (Event::Resize(80, 24), None),
        ];
        for (event, expected) in table {
            assert_eq!(action_for(&event), expected, "{event:?}");
        }
    }

    #[test]
    fn key_release_is_ignored() {
        let mut k = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        assert_eq!(action_for(&Event::Key(k)), None);
    }

    #[test]
    fn quit_sets_flag() {
        let (mut app, _) = app();
        app.update(Action::Quit);
        assert!(app.should_quit());
    }
}
