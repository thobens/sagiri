//! Application state. No terminal or clipboard code lives here.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sagiri_core::engine::{Engine, Finding, render};
use sagiri_core::rule::Kind;

use super::clipboard::Clipboard;
use super::ui::line_width;

/// Lines moved by a page scroll when the viewport size is not known yet.
const DEFAULT_PAGE: usize = 10;
/// Columns of a text pane when the size is not known yet.
const DEFAULT_WIDTH: usize = 40;
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
    /// Switch focus between the text panes and the findings panel.
    FocusNext,
    /// Switch the selected findings entry on or off.
    ToggleEntry,
    /// Switch every entry of the selected entry's kind on or off.
    ToggleKind,
    /// Switch everything back on.
    EnableAll,
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

/// Which area gets the arrow keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Text,
    Findings,
}

/// A highlighted byte range: a finding in the original or its text in the output.
#[derive(Clone, Debug)]
pub struct Mark {
    pub range: Range<usize>,
    pub kind: Kind,
    /// False if the finding is switched off.
    pub on: bool,
}

/// One row of the findings panel: a distinct (kind, original text).
#[derive(Clone)]
pub struct Entry {
    pub kind: Kind,
    pub original: String,
    pub replacement: String,
    pub count: usize,
    /// Index of the first occurrence in the findings.
    first: usize,
    /// False if the entry is switched off.
    pub on: bool,
}

/// One text split into lines, for cheap access to the visible ones.
#[derive(Default)]
struct Lines {
    text: String,
    ranges: Vec<Range<usize>>,
    max_width: usize,
    /// Sorted by start, without overlaps.
    marks: Vec<Mark>,
}

impl Lines {
    fn new(text: String, marks: Vec<Mark>) -> Self {
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
            marks,
        }
    }

    /// Marks that fall on a line, with ranges relative to the start of the line.
    fn line_marks(&self, index: usize) -> Vec<Mark> {
        let Some(r) = self.ranges.get(index) else {
            return Vec::new();
        };
        let first = self.marks.partition_point(|m| m.range.end <= r.start);
        self.marks[first..]
            .iter()
            .take_while(|m| m.range.start < r.end)
            .map(|m| Mark {
                range: m.range.start - r.start..m.range.end - r.start,
                ..m.clone()
            })
            .collect()
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
    findings: Vec<Finding>,
    entries: Vec<Entry>,
    /// Entry index of every finding.
    finding_entry: Vec<usize>,
    selected: usize,
    focus: Focus,
    /// Decisions kept in memory until exit. Never written anywhere.
    off_entries: HashSet<(Kind, String)>,
    off_kinds: HashSet<Kind>,
    /// Entries switched on again inside a switched-off kind.
    on_entries: HashSet<(Kind, String)>,
    v_offset: usize,
    h_offset: usize,
    page: usize,
    pane_width: usize,
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
            findings: Vec::new(),
            entries: Vec::new(),
            finding_entry: Vec::new(),
            selected: 0,
            focus: Focus::Text,
            off_entries: HashSet::new(),
            off_kinds: HashSet::new(),
            on_entries: HashSet::new(),
            pane_width: DEFAULT_WIDTH,
            v_offset: 0,
            h_offset: 0,
            page: DEFAULT_PAGE,
            message: None,
            quit: false,
        }
    }

    /// Tells the app how many columns a text pane shows, for scrolling to a finding.
    pub fn set_pane_width(&mut self, columns: usize) {
        self.pane_width = columns.max(1);
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
            Action::FocusNext => {
                self.focus = match self.focus {
                    Focus::Findings => Focus::Text,
                    Focus::Text if !self.entries.is_empty() => Focus::Findings,
                    Focus::Text => Focus::Text,
                }
            }
            Action::ToggleEntry if self.focus == Focus::Findings => self.toggle_entry(),
            Action::ToggleKind if self.focus == Focus::Findings => self.toggle_kind(),
            Action::ToggleEntry | Action::ToggleKind => {}
            Action::EnableAll => {
                self.off_entries.clear();
                self.off_kinds.clear();
                self.on_entries.clear();
                self.refresh();
            }
            Action::ScrollUp if self.focus == Focus::Findings => self.select(-1),
            Action::ScrollDown if self.focus == Focus::Findings => self.select(1),
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
        self.findings = self.engine.analyze(&text);
        self.build_entries();
        self.original = Lines::new(text, Vec::new());
        self.selected = 0;
        if self.entries.is_empty() {
            self.focus = Focus::Text;
        }
        self.refresh();
        self.v_offset = 0;
        self.h_offset = 0;
    }

    /// Groups the findings into entries, in order of first appearance.
    fn build_entries(&mut self) {
        let mut index: HashMap<(Kind, &str), usize> = HashMap::new();
        let mut entries: Vec<Entry> = Vec::new();
        let mut finding_entry = Vec::with_capacity(self.findings.len());
        for (i, f) in self.findings.iter().enumerate() {
            let e = *index.entry((f.kind, &f.original)).or_insert_with(|| {
                entries.push(Entry {
                    kind: f.kind,
                    original: f.original.clone(),
                    replacement: f.replacement.clone(),
                    count: 0,
                    first: i,
                    on: true,
                });
                entries.len() - 1
            });
            entries[e].count += 1;
            finding_entry.push(e);
        }
        self.entries = entries;
        self.finding_entry = finding_entry;
    }

    /// Applies the remembered decisions to the entries and re-renders the output. Does not
    /// analyze again.
    fn refresh(&mut self) {
        let on: Vec<bool> = self
            .entries
            .iter()
            .map(|e| self.is_on(e.kind, &e.original))
            .collect();
        for (e, on) in self.entries.iter_mut().zip(on) {
            e.on = on;
        }
        let on = |f: &Finding| {
            self.findings
                .binary_search_by_key(&f.span.start, |x| x.span.start)
                .map_or(true, |i| self.entries[self.finding_entry[i]].on)
        };
        let rendered = render(&self.original.text, &self.findings, on);
        let original_marks = self
            .findings
            .iter()
            .zip(&self.finding_entry)
            .map(|(f, &e)| Mark {
                range: f.span.clone(),
                kind: f.kind,
                on: self.entries[e].on,
            })
            .collect();
        let output_marks = rendered
            .spans
            .iter()
            .map(|s| Mark {
                range: s.range.clone(),
                kind: self.findings[s.finding].kind,
                on: s.replaced,
            })
            .collect();
        self.original.marks = original_marks;
        self.output = Lines::new(rendered.text, output_marks);
    }

    fn select(&mut self, delta: isize) {
        let last = self.entries.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.reveal_selected();
    }

    /// Scrolls so that the first occurrence of the selected entry is visible.
    fn reveal_selected(&mut self) {
        let Some(e) = self.entries.get(self.selected) else {
            return;
        };
        let pos = self.findings[e.first].span.start;
        let line = self
            .original
            .ranges
            .partition_point(|r| r.start <= pos)
            .saturating_sub(1);
        if line < self.v_offset || line >= self.v_offset + self.page {
            self.v_offset = line.saturating_sub(self.page / 2).min(self.max_v());
        }
        // Horizontal: the occurrence in both panes (the replacement can sit at another
        // column than the original) must lie inside the visible columns.
        let col_in = |lines: &Lines, pos: usize| {
            let start = lines.ranges.get(line).map_or(0, |r| r.start);
            let end = pos.min(lines.text.len());
            lines.text.get(start..end).map_or(0, super::ui::line_width)
        };
        let o_start = col_in(&self.original, pos);
        let o_end = o_start + line_width(&e.original);
        let out_pos = self.output.marks.get(e.first).map(|m| m.range.start);
        let out_start = out_pos.map_or(o_start, |p| col_in(&self.output, p));
        let out_end = out_start + line_width(&self.output_mark_text(e.first));
        let (lo, hi) = (o_start.min(out_start), o_end.max(out_end));
        let width = self.pane_width;
        if lo < self.h_offset || hi > self.h_offset + width {
            self.h_offset = lo.saturating_sub(width / 4).min(self.max_h());
        }
    }

    fn output_mark_text(&self, finding: usize) -> String {
        self.output
            .marks
            .get(finding)
            .and_then(|m| self.output.text.get(m.range.clone()))
            .unwrap_or("")
            .to_string()
    }

    /// Whether the entry is replaced, given the remembered decisions.
    fn is_on(&self, kind: Kind, original: &str) -> bool {
        let key = (kind, original.to_string());
        !self.off_entries.contains(&key)
            && (!self.off_kinds.contains(&kind) || self.on_entries.contains(&key))
    }

    fn toggle_entry(&mut self) {
        let Some(e) = self.entries.get(self.selected) else {
            return;
        };
        let kind = e.kind;
        let key = (kind, e.original.clone());
        let kind_off = self.off_kinds.contains(&kind);
        if e.on {
            if !self.on_entries.remove(&key) {
                self.off_entries.insert(key);
            }
        } else {
            self.off_entries.remove(&key);
            if kind_off {
                // An exception to the switched-off kind; the kind stays off for new values.
                self.on_entries.insert(key);
            }
        }
        self.refresh();
    }

    fn toggle_kind(&mut self) {
        let Some(kind) = self.entries.get(self.selected).map(|e| e.kind) else {
            return;
        };
        let all_off = self
            .entries
            .iter()
            .filter(|e| e.kind == kind)
            .all(|e| !e.on);
        if all_off {
            self.off_kinds.remove(&kind);
        } else {
            self.off_kinds.insert(kind);
        }
        // The kind decision now covers the values on screen. Decisions about values that
        // are not on screen stay remembered.
        for e in self.entries.iter().filter(|e| e.kind == kind) {
            let key = (kind, e.original.clone());
            self.off_entries.remove(&key);
            self.on_entries.remove(&key);
        }
        self.on_entries.retain(|(k, _)| *k != kind);
        self.refresh();
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

    #[cfg(test)]
    pub fn finding_count(&self) -> usize {
        self.findings.len()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// Entries that are replaced and entries that are kept.
    pub fn status_counts(&self) -> (usize, usize) {
        let replaced = self.entries.iter().filter(|e| e.on).count();
        (replaced, self.entries.len() - replaced)
    }

    pub fn original_marks(&self, line: usize) -> Vec<Mark> {
        self.original.line_marks(line)
    }

    pub fn output_marks(&self, line: usize) -> Vec<Mark> {
        self.output.line_marks(line)
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
        KeyCode::Tab | KeyCode::BackTab => Some(Action::FocusNext),
        KeyCode::Char(' ') => Some(Action::ToggleEntry),
        KeyCode::Char('t') => Some(Action::ToggleKind),
        KeyCode::Char('a') => Some(Action::EnableAll),
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
    fn page_scroll_uses_page_size() {
        let (mut app, _) = app();
        paste(&mut app, "1\n2\n3\n4\n5\n6\n7\n8");
        app.set_page_size(3);
        app.update(Action::PageDown);
        assert_eq!(app.v_offset(), 3);
        app.update(Action::PageDown);
        app.update(Action::PageDown);
        assert_eq!(app.v_offset(), 7);
        app.update(Action::PageUp);
        assert_eq!(app.v_offset(), 4);
        app.set_page_size(0);
        app.update(Action::PageUp);
        assert_eq!(app.v_offset(), 3);
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
            (key(KeyCode::Tab), Some(Action::FocusNext)),
            (ch(' '), Some(Action::ToggleEntry)),
            (ch('t'), Some(Action::ToggleKind)),
            (ch('a'), Some(Action::EnableAll)),
            (ctrl('l'), Some(Action::Clear)),
            (key(KeyCode::Up), Some(Action::ScrollUp)),
            (ch('k'), Some(Action::ScrollUp)),
            (key(KeyCode::Down), Some(Action::ScrollDown)),
            (ch('j'), Some(Action::ScrollDown)),
            (key(KeyCode::PageUp), Some(Action::PageUp)),
            (key(KeyCode::PageDown), Some(Action::PageDown)),
            (ch('g'), Some(Action::Top)),
            (ch('G'), Some(Action::Bottom)),
            (
                Event::Key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
                Some(Action::Bottom),
            ),
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

    const A: &str = "172.55.0.1";
    const B: &str = "198.51.100.7";

    fn focus_findings(app: &mut App) {
        app.update(Action::FocusNext);
        assert_eq!(app.focus(), Focus::Findings);
    }

    #[test]
    fn entries_group_by_kind_and_original() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B} {A}\n{A}"));
        let e = app.entries();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].original.as_str(), e[0].count), (A, 3));
        assert_eq!((e[1].original.as_str(), e[1].count), (B, 1));
        assert_eq!(e[0].kind, Kind::Ipv4);
    }

    #[test]
    fn toggle_entry_keeps_original_in_output() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} x {B} y {A}"));
        let before = app.output().to_string();
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        assert_eq!(app.output().matches(A).count(), 2);
        assert!(!app.output().contains(B));
        app.update(Action::ToggleEntry);
        assert_eq!(app.output(), before);
    }

    struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl sagiri_core::rule::Rule for Counting {
        fn kind(&self) -> Kind {
            Kind::Term
        }
        fn detect(&self, text: &str) -> Vec<Range<usize>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            text.match_indices("SECRET")
                .map(|(i, m)| i..i + m.len())
                .collect()
        }
        fn replace(&self, _: &str, _: &sagiri_core::rule::Context<'_>) -> String {
            "XXXXXX".to_string()
        }
    }

    #[test]
    fn toggle_does_not_reanalyze() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let engine = Engine::new(
            Pseudonymizer::new(&Key::from_bytes([7; 32])),
            vec![Box::new(Counting(calls.clone()))],
        );
        let mut app = App::new(engine, Box::new(FakeClipboard::default()));
        paste(&mut app, "a SECRET b");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(app.output(), "a XXXXXX b");
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        assert_eq!(app.output(), "a SECRET b");
        app.update(Action::ToggleKind);
        app.update(Action::EnableAll);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn toggle_kind_affects_all_entries_of_that_kind() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B} main.acme.com"));
        assert_eq!(app.entries().len(), 3);
        focus_findings(&mut app);
        app.update(Action::ToggleKind);
        assert!(app.output().contains(A) && app.output().contains(B));
        assert!(!app.output().contains("main.acme.com"));
        assert_eq!(app.status_counts(), (1, 2));
        app.update(Action::ToggleKind);
        assert_eq!(app.status_counts(), (3, 0));
        assert!(!app.output().contains(A));
    }

    #[test]
    fn enable_all_restores_everything() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B}"));
        let before = app.output().to_string();
        focus_findings(&mut app);
        app.update(Action::ToggleKind);
        assert_eq!(app.output(), app.original());
        app.update(Action::EnableAll);
        assert_eq!(app.output(), before);
        assert_eq!(app.status_counts(), (2, 0));
        // The memory is gone for new pastes too.
        app.update(Action::ToggleEntry);
        app.update(Action::FocusNext);
        app.update(Action::EnableAll);
        paste(&mut app, A);
        assert_eq!(app.status_counts(), (1, 0));
    }

    #[test]
    fn copy_respects_toggles() {
        let (mut app, clip) = app();
        paste(&mut app, &format!("{A} {B}"));
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        app.update(Action::Copy);
        let copied = clip.content();
        assert!(copied.starts_with(A));
        assert!(!copied.contains(B));
        assert_eq!(copied, app.output());
    }

    #[test]
    fn decisions_survive_new_paste_and_clear() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B}"));
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        paste(&mut app, &format!("{B} {A}"));
        assert_eq!(app.status_counts(), (1, 1));
        assert!(app.output().contains(A));
        app.update(Action::Clear);
        paste(&mut app, &format!("zz {A}"));
        assert_eq!(app.output(), format!("zz {A}"));
        assert_eq!(app.status_counts(), (0, 1));
    }

    #[test]
    fn kind_decision_applies_to_later_pastes() {
        let (mut app, _) = app();
        paste(&mut app, A);
        focus_findings(&mut app);
        app.update(Action::ToggleKind);
        paste(&mut app, B);
        assert_eq!(app.output(), B);
    }

    #[test]
    fn entry_toggle_inside_a_switched_off_kind() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B}"));
        focus_findings(&mut app);
        app.update(Action::ToggleKind);
        app.update(Action::ToggleEntry);
        assert_eq!(app.status_counts(), (1, 1));
        assert!(!app.output().contains(A));
        assert!(app.output().contains(B));
    }

    #[test]
    fn selection_scrolls_to_first_occurrence() {
        let (mut app, _) = app();
        let mut text = format!("{A}\n");
        for _ in 0..498 {
            text.push_str("filler\n");
        }
        text.push_str(&format!("{B}\n"));
        for _ in 0..400 {
            text.push_str("filler\n");
        }
        // A second occurrence far below the first one.
        text.push_str(&format!("{B}\nend"));
        paste(&mut app, &text);
        app.set_page_size(10);
        focus_findings(&mut app);
        assert_eq!(app.v_offset(), 0);
        app.update(Action::ScrollDown);
        assert_eq!(app.selected(), 1);
        let line = 499;
        assert!(app.v_offset() <= line && line < app.v_offset() + 10);
        assert_eq!(app.original_line(line), B);
        app.update(Action::ScrollUp);
        assert_eq!(app.v_offset(), 0);
    }

    #[test]
    fn selection_scrolls_horizontally_to_the_occurrence() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A}\n{}{B}\n", "x ".repeat(150)));
        app.set_pane_width(40);
        focus_findings(&mut app);
        app.update(Action::ScrollDown);
        let (h, start) = (app.h_offset(), 300);
        assert!(h <= start && start + B.len() <= h + 40, "{h}");
        app.update(Action::ScrollUp);
        assert_eq!(app.h_offset(), 0);
        // Scrolled right already: selecting a value at the start scrolls back.
        app.update(Action::FocusNext);
        app.update(Action::ScrollRight);
        app.update(Action::FocusNext);
        app.update(Action::ScrollDown);
        app.update(Action::ScrollUp);
        assert_eq!(app.h_offset(), 0);
    }

    #[test]
    fn kind_decision_survives_entry_toggle_in_it() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B}"));
        focus_findings(&mut app);
        app.update(Action::ToggleKind);
        app.update(Action::ToggleEntry);
        // A is an exception; a new value of the kind is still kept.
        paste(&mut app, &format!("{A} {B} 203.0.113.9"));
        assert_eq!(app.status_counts(), (1, 2));
        assert!(!app.output().contains(A));
        assert!(app.output().contains("203.0.113.9"));
        // And the exception can be undone.
        app.update(Action::ToggleEntry);
        assert_eq!(app.status_counts(), (0, 3));
    }

    #[test]
    fn switching_a_kind_on_keeps_decisions_for_values_not_on_screen() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A} {B}"));
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        paste(&mut app, B);
        app.update(Action::ToggleKind);
        app.update(Action::ToggleKind);
        paste(&mut app, &format!("{A} {B}"));
        assert_eq!(app.status_counts(), (1, 1));
        assert!(app.output().contains(A));
        assert!(!app.output().contains(B));
    }

    #[test]
    fn status_counts() {
        let (mut app, _) = app();
        assert_eq!(app.status_counts(), (0, 0));
        paste(&mut app, &format!("{A} {B} {A}"));
        assert_eq!(app.status_counts(), (2, 0));
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        assert_eq!(app.status_counts(), (1, 1));
    }

    #[test]
    fn focus_changes_arrow_key_meaning() {
        let (mut app, _) = app();
        paste(&mut app, &format!("{A}\nline\n{B}\nmore"));
        app.update(Action::ScrollDown);
        assert_eq!((app.v_offset(), app.selected()), (1, 0));
        app.update(Action::Top);
        focus_findings(&mut app);
        app.update(Action::ScrollDown);
        assert_eq!((app.v_offset(), app.selected()), (0, 1));
        app.update(Action::ScrollDown);
        assert_eq!(app.selected(), 1);
        app.update(Action::ScrollUp);
        assert_eq!(app.selected(), 0);
        app.update(Action::FocusNext);
        assert_eq!(app.focus(), Focus::Text);
        app.update(Action::ScrollDown);
        assert_eq!((app.v_offset(), app.selected()), (1, 0));
    }

    #[test]
    fn toggles_need_the_findings_focus_and_focus_needs_entries() {
        let (mut app, _) = app();
        app.update(Action::FocusNext);
        assert_eq!(app.focus(), Focus::Text);
        paste(&mut app, A);
        app.update(Action::ToggleEntry);
        assert_eq!(app.status_counts(), (1, 0));
    }
}
