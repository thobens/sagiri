//! Pure rendering. Only the visible lines are turned into widgets.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthChar;

use super::app::App;

/// Terminals at least this wide get the panes side by side.
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 100;
const EMPTY_HINT: &str = "Paste text here (terminal paste or Ctrl+V)";
const KEY_HINTS: &str = "y copy  p paste  x clear  q quit";
const TAB_WIDTH: usize = 4;

/// Control characters and invisible format characters (bidi overrides, zero-width marks)
/// are shown as `·`, so that they cannot act on the terminal or disguise text.
fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}')
}

/// Columns a char takes on screen after the display rules (a tab is four spaces, any other
/// control or invisible format character is one `·`).
fn char_width(c: char) -> usize {
    match c {
        '\t' => TAB_WIDTH,
        c if is_unsafe(c) => 1,
        c => c.width().unwrap_or(0),
    }
}

/// Screen width of a line after the display rules.
pub fn line_width(line: &str) -> usize {
    line.chars().map(char_width).sum()
}

/// The part of `line` that is visible with `skip` columns scrolled off to the left and
/// `width` columns available, with the display rules applied. Work is bounded by
/// `skip + width`, not by the length of the line.
pub fn visible_part(line: &str, skip: usize, width: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    let mut used = 0;
    for c in line.chars() {
        if col >= skip + width {
            break;
        }
        let w = char_width(c);
        let start = col;
        col += w;
        if col <= skip {
            continue;
        }
        let remaining = width - used;
        if start < skip {
            // A wide char or a tab cut by the left edge: show the rest as blanks.
            let n = (col - skip).min(remaining);
            out.extend(std::iter::repeat_n(' ', n));
            used += n;
        } else if w > remaining {
            break;
        } else if c == '\t' {
            out.push_str("    ");
            used += w;
        } else if is_unsafe(c) {
            out.push('·');
            used += 1;
        } else {
            out.push(c);
            used += w;
        }
    }
    out
}

pub fn draw(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let [panes, status] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let direction = if area.width >= SIDE_BY_SIDE_MIN_WIDTH {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };
    let [left, right] = Layout::default()
        .direction(direction)
        .constraints([Constraint::Fill(1), Constraint::Fill(1)])
        .areas(panes);
    draw_pane(app, frame, left, "Original", |i| app.original_line(i));
    draw_pane(app, frame, right, "Obfuscated", |i| app.output_line(i));
    draw_status(app, frame, status);
}

fn draw_pane<'a>(
    app: &'a App,
    frame: &mut Frame,
    area: Rect,
    title: &str,
    line: impl Fn(usize) -> &'a str,
) {
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    let content: Vec<Line> = if app.is_empty() {
        vec![Line::styled(
            EMPTY_HINT,
            Style::new().add_modifier(Modifier::DIM),
        )]
    } else {
        let first = app.v_offset();
        let last = app.line_count().min(first + usize::from(inner.height));
        (first..last)
            .map(|i| {
                Line::raw(visible_part(
                    line(i),
                    app.h_offset(),
                    usize::from(inner.width),
                ))
            })
            .collect()
    };
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(content), inner);
}

fn draw_status(app: &App, frame: &mut Frame, area: Rect) {
    let left_text = match app.message() {
        Some(m) => m.to_string(),
        None => {
            let n = app.finding_count();
            let l = app.line_count();
            format!(
                "{n} {}, {l} {}",
                if n == 1 { "finding" } else { "findings" },
                if l == 1 { "line" } else { "lines" }
            )
        }
    };
    // The key hints are ASCII, so chars equal columns. They are left out when the status
    // line is too narrow to hold them next to the message (52 columns or fewer).
    let hints_width = KEY_HINTS.chars().count() as u16;
    if area.width > hints_width + 20 {
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(hints_width)]).areas(area);
        frame.render_widget(Paragraph::new(left_text), left);
        frame.render_widget(Paragraph::new(KEY_HINTS), right);
    } else {
        frame.render_widget(Paragraph::new(left_text), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::tests::app;
    use crate::tui::app::{Action, App};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn render(app: &App, w: u16, h: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(app, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height).map(|y| row(buf, y)).collect()
    }

    fn find(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
        rows(buf)
            .iter()
            .enumerate()
            .find_map(|(y, r)| r.find(needle).map(|x| (x as u16, y as u16)))
    }

    fn pasted(text: &str) -> App {
        let (mut app, _) = app();
        app.update(Action::Paste(text.to_string()));
        app
    }

    #[test]
    fn layout_depends_on_width() {
        let (app, _) = app();
        let wide = render(&app, 120, 30);
        let (ox, oy) = find(&wide, "Original").unwrap();
        let (bx, by) = find(&wide, "Obfuscated").unwrap();
        assert_eq!(oy, by);
        assert!(bx > ox);

        let narrow = render(&app, 80, 30);
        let (ox, oy) = find(&narrow, "Original").unwrap();
        let (bx, by) = find(&narrow, "Obfuscated").unwrap();
        assert_eq!(ox, bx);
        assert!(by > oy);

        let edge = render(&app, 100, 30);
        assert_eq!(
            find(&edge, "Original").unwrap().1,
            find(&edge, "Obfuscated").unwrap().1
        );
        let below = render(&app, 99, 30);
        assert_ne!(
            find(&below, "Original").unwrap().1,
            find(&below, "Obfuscated").unwrap().1
        );
    }

    #[test]
    fn empty_state_shows_hint() {
        let (app, _) = app();
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "Paste text here (terminal paste or Ctrl+V)").is_some());
    }

    #[test]
    fn paste_is_shown_in_both_panes_with_status() {
        let app = pasted("server 172.55.0.1 is down");
        let replacement = app.output().to_string();
        assert_ne!(replacement, app.original());
        let buf = render(&app, 120, 30);
        let (ox, _) = find(&buf, "server 172.55.0.1 is down").unwrap();
        let (bx, by) = find(&buf, &replacement).unwrap();
        assert!(ox < 60 && bx >= 60);
        assert_eq!(by, 1);
        assert!(find(&buf, "1 finding, 1 line").is_some());
        assert!(find(&buf, "Paste text here").is_none());
    }

    #[test]
    fn status_line_shows_message() {
        let mut app = pasted("a 172.55.0.1");
        app.update(Action::Copy);
        // The real clipboard is never touched: the fake in `app()` takes the text.
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "Copied ").is_some());
    }

    #[test]
    fn control_characters_are_not_rendered() {
        let app = pasted("\x1b[31mred\x1b[0m\tend");
        for w in [120, 80] {
            let buf = render(&app, w, 30);
            for y in 0..buf.area.height {
                let r = row(&buf, y);
                assert!(!r.contains('\x1b'), "{r:?}");
                assert!(!r.contains('\t'), "{r:?}");
            }
            assert!(find(&buf, "·[31mred·[0m    end").is_some());
        }
        // The text itself is untouched.
        assert!(app.original().contains('\x1b'));
    }

    #[test]
    fn wide_characters_do_not_shift_columns() {
        let app = pasted("日本語X");
        let buf = render(&app, 120, 30);
        // Left pane content starts at column 1 (inside the border); X follows 6 columns.
        let x = (0..60u16).find(|&x| buf[(x, 1)].symbol() == "X").unwrap();
        assert_eq!(x, 1 + 6);
        let x = (60..120u16).find(|&x| buf[(x, 1)].symbol() == "X").unwrap();
        assert_eq!(x, 60 + 1 + 6);
    }

    #[test]
    fn format_characters_are_not_rendered() {
        let app = pasted("a\u{202e}b\u{200b}c\u{61c}d\u{180e}");
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "a·b·c·d·").is_some());
        assert!(!rows(&buf).iter().any(|r| r.contains('\u{202e}')));
    }

    #[test]
    fn visible_part_scrolls_and_crops() {
        assert_eq!(visible_part("abcdef", 2, 3), "cde");
        assert_eq!(visible_part("abcdef", 0, 100), "abcdef");
        assert_eq!(visible_part("日本語", 1, 10), " 本語");
        assert_eq!(visible_part("日本語", 0, 3), "日");
        assert_eq!(visible_part("a\tb", 2, 10), "   b");
        assert_eq!(visible_part("abc", 10, 5), "");
    }

    #[test]
    fn scrolling_shows_same_lines_in_both_panes() {
        let mut app = pasted("one 172.55.0.1\ntwo 172.55.0.2\nthree 172.55.0.3");
        app.update(Action::ScrollDown);
        app.update(Action::ScrollDown);
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "three 172.55.0.3").is_some());
        assert!(find(&buf, "one ").is_none());
        assert!(find(&buf, "two ").is_none());
    }

    #[test]
    fn large_paste_draws_only_visible_lines() {
        let line = "host 172.55.0.1 says hello to the world and more text\n";
        let text = line.repeat(1_048_576 / line.len());
        let mut app = pasted(&text);
        for _ in 0..5 {
            app.update(Action::PageDown);
        }
        let start = std::time::Instant::now();
        let buf = render(&app, 120, 40);
        assert!(find(&buf, "says hello").is_some());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
}
