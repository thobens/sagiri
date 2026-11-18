//! Pure rendering. Only the visible lines are turned into widgets.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use sagiri_core::rule::Kind;
use unicode_width::UnicodeWidthChar;

use super::app::{App, Entry, Focus, Mark};

/// Terminals at least this wide get the panes side by side.
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 100;
const EMPTY_HINT: &str = "Paste text here (terminal paste or Ctrl+V)";
const KEY_HINTS: &str = "y copy  p paste  x clear  Tab focus  Space toggle  t kind  a all  q quit";
const TAB_WIDTH: usize = 4;

/// Control characters and invisible format characters (bidi overrides, zero-width marks)
/// are shown as `·`, so that they cannot act on the terminal or disguise text.
fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{ad}' | '\u{34f}' | '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{e0000}'..='\u{e007f}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
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

/// The colour of a kind, from the 16-colour ANSI palette. One per kind, none shared.
pub fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::Term => Color::Magenta,
        Kind::Url => Color::Blue,
        Kind::Email => Color::Cyan,
        Kind::Ipv4 => Color::Green,
        Kind::Ipv6 => Color::LightGreen,
        Kind::Hostname => Color::Yellow,
        Kind::PaymentCard => Color::Red,
        Kind::Iban => Color::LightRed,
        Kind::Phone => Color::LightBlue,
    }
}

/// Replaced findings are bold in the colour of their kind, kept ones dim and underlined.
pub fn mark_style(kind: Kind, on: bool) -> Style {
    let style = Style::new().fg(kind_color(kind));
    if on {
        style.add_modifier(Modifier::BOLD)
    } else {
        style.add_modifier(Modifier::DIM | Modifier::UNDERLINED)
    }
}

/// The visible cells of `line`: `skip` columns scrolled off to the left, `width` columns
/// available, display rules applied. `marks` (relative to the line, sorted) give the style.
/// Work is bounded by `skip + width`, not by the length of the line.
fn visible_cells(line: &str, marks: &[Mark], skip: usize, width: usize) -> Vec<(char, Style)> {
    let mut out = Vec::new();
    let mut col = 0;
    let mut used = 0;
    let mut mark = 0;
    for (byte, c) in line.char_indices() {
        if col >= skip + width {
            break;
        }
        while mark < marks.len() && marks[mark].range.end <= byte {
            mark += 1;
        }
        let style = match marks.get(mark) {
            Some(m) if m.range.start <= byte => mark_style(m.kind, m.on),
            _ => Style::new(),
        };
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
            out.extend(std::iter::repeat_n((' ', style), n));
            used += n;
        } else if w > remaining {
            break;
        } else if c == '\t' {
            out.extend(std::iter::repeat_n((' ', style), TAB_WIDTH));
            used += w;
        } else if is_unsafe(c) {
            out.push(('·', style));
            used += 1;
        } else {
            out.push((c, style));
            used += w;
        }
    }
    out
}

/// The part of `line` that is visible, as plain text.
#[cfg(test)]
pub fn visible_part(line: &str, skip: usize, width: usize) -> String {
    visible_cells(line, &[], skip, width)
        .into_iter()
        .map(|(c, _)| c)
        .collect()
}

/// The visible part of `line` as styled spans.
fn visible_spans(line: &str, marks: &[Mark], skip: usize, width: usize) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text = String::new();
    let mut style = Style::new();
    for (c, s) in visible_cells(line, marks, skip, width) {
        if s != style && !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), style));
        }
        style = s;
        text.push(c);
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    spans
}

/// Replaces tabs and unsafe characters so that a value can be shown in the findings panel.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' => ' ',
            c if is_unsafe(c) => '·',
            c => c,
        })
        .collect()
}

/// Shortens `s` in the middle with `…` to at most `max` columns.
fn shorten_middle(s: &str, max: usize) -> String {
    if line_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max - 1;
    let tail_cols = keep / 2;
    let head_cols = keep - tail_cols;
    let mut head = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w > head_cols {
            break;
        }
        head.push(c);
        used += w;
    }
    let mut tail: Vec<char> = Vec::new();
    used = 0;
    for c in s.chars().rev() {
        let w = char_width(c);
        if used + w > tail_cols {
            break;
        }
        tail.push(c);
        used += w;
    }
    tail.reverse();
    format!("{head}…{}", tail.into_iter().collect::<String>())
}

/// Shortens two values so that together they fit `budget` columns.
fn fit_pair(a: &str, b: &str, budget: usize) -> (String, String) {
    let (wa, wb) = (line_width(a), line_width(b));
    let (ma, mb) = if wa + wb <= budget {
        (wa, wb)
    } else if wa <= budget / 2 {
        (wa, budget - wa)
    } else if wb <= budget - budget / 2 {
        (budget - wb, wb)
    } else {
        (budget / 2, budget - budget / 2)
    };
    (shorten_middle(a, ma), shorten_middle(b, mb))
}

struct Areas {
    left: Rect,
    right: Rect,
    panel: Rect,
    status: Rect,
}

fn areas(area: Rect, entries: usize) -> Areas {
    let panel_height = if entries == 0 {
        0
    } else {
        (entries.min(usize::from(u16::MAX) - 2) as u16 + 2).min(area.height / 3)
    };
    let [panes, panel, status] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(panel_height),
        Constraint::Length(1),
    ])
    .areas(area);
    let direction = if area.width >= SIDE_BY_SIDE_MIN_WIDTH {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };
    let [left, right] = Layout::default()
        .direction(direction)
        .constraints([Constraint::Fill(1), Constraint::Fill(1)])
        .areas(panes);
    Areas {
        left,
        right,
        panel,
        status,
    }
}

/// Lines that fit in a text pane on a screen of `area`, for page scrolling.
pub fn page_size(area: Rect, entries: usize) -> usize {
    let a = areas(area, entries);
    usize::from(a.left.height.min(a.right.height).saturating_sub(2))
}

pub fn draw(app: &App, frame: &mut Frame) {
    let a = areas(frame.area(), app.entries().len());
    let text_focus = app.focus() == Focus::Text;
    draw_pane(app, frame, a.left, "Original", text_focus, |i| {
        (app.original_line(i), app.original_marks(i))
    });
    draw_pane(app, frame, a.right, "Obfuscated", text_focus, |i| {
        (app.output_line(i), app.output_marks(i))
    });
    draw_panel(app, frame, a.panel);
    draw_status(app, frame, a.status);
}

fn focus_block(title: &str, focused: bool) -> Block<'_> {
    let block = Block::bordered().title(title);
    if focused {
        block
            .border_type(BorderType::Thick)
            .border_style(Style::new().add_modifier(Modifier::BOLD))
    } else {
        block
    }
}

fn draw_pane<'a>(
    app: &'a App,
    frame: &mut Frame,
    area: Rect,
    title: &str,
    focused: bool,
    line: impl Fn(usize) -> (&'a str, Vec<Mark>),
) {
    let block = focus_block(title, focused);
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
                let (text, marks) = line(i);
                Line::from(visible_spans(
                    text,
                    &marks,
                    app.h_offset(),
                    usize::from(inner.width),
                ))
            })
            .collect()
    };
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(content), inner);
}

fn draw_panel(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let focused = app.focus() == Focus::Findings;
    let block = focus_block("Findings", focused);
    let inner = block.inner(area);
    let rows = usize::from(inner.height);
    let offset = (app.selected() + 1).saturating_sub(rows);
    let lines: Vec<Line> = app
        .entries()
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .map(|(i, e)| entry_line(e, usize::from(inner.width), focused && i == app.selected()))
        .collect();
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `[x] IPv4       172.55.0.1 → 126.143.4.12   ×3`, shortened to `width` columns.
fn entry_line(e: &Entry, width: usize, selected: bool) -> Line<'static> {
    let count = format!("  ×{}", e.count);
    let label = format!("{:<8} ", e.kind.label());
    let fixed = 4 + line_width(&label) + line_width(&count);
    let budget = width.saturating_sub(fixed);
    let mut spans = vec![
        Span::raw(if e.on { "[x] " } else { "[ ] " }),
        Span::styled(label, Style::new().fg(kind_color(e.kind))),
    ];
    if budget > 3 {
        let (o, r) = fit_pair(
            &sanitize(&e.original),
            &sanitize(&e.replacement),
            budget - 3,
        );
        spans.push(Span::raw(format!("{o} → {r}")));
        spans.push(Span::raw(count));
    }
    let line = Line::from(spans);
    if selected {
        line.style(Style::new().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

fn draw_status(app: &App, frame: &mut Frame, area: Rect) {
    let left_text = match app.message() {
        Some(m) => m.to_string(),
        None => {
            let (replaced, kept) = app.status_counts();
            let l = app.line_count();
            format!(
                "{replaced} replaced · {kept} kept, {l} {}",
                if l == 1 { "line" } else { "lines" }
            )
        }
    };
    // The key hints are ASCII, so chars equal columns. They are left out when the status
    // line is too narrow to hold them next to the message.
    let hints_width = KEY_HINTS.chars().count() as u16;
    if area.width > hints_width + 36 {
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
    use ratatui::style::Color;
    use sagiri_core::rule::Kind;

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
        assert!(find(&buf, "1 replaced · 0 kept, 1 line").is_some());
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
        let app = pasted("a\u{202e}b\u{200b}c\u{61c}d\u{180e}\u{34f}\u{3164}\u{e0041}");
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "a·b·c·d····").is_some());
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

    fn style_at(buf: &Buffer, x: u16, y: u16) -> Style {
        buf[(x, y)].style()
    }

    /// Position of `needle` in the buffer, searching from column `min_x`.
    fn find_from(buf: &Buffer, needle: &str, min_x: u16) -> Option<(u16, u16)> {
        rows(buf).iter().enumerate().find_map(|(y, r)| {
            let chars: Vec<char> = r.chars().collect();
            let hay: String = chars[usize::from(min_x)..].iter().collect();
            hay.find(needle)
                .map(|b| (min_x + hay[..b].chars().count() as u16, y as u16))
        })
    }

    fn assert_cells(buf: &Buffer, (x, y): (u16, u16), len: usize, f: impl Fn(Style) -> bool) {
        for i in 0..len as u16 {
            assert!(f(style_at(buf, x + i, y)), "cell {} of {len}", i);
        }
    }

    fn focus_findings(app: &mut App) {
        app.update(Action::FocusNext);
    }

    #[test]
    fn findings_are_highlighted_in_both_panes() {
        let app = pasted("server 172.55.0.1 is down");
        let replacement = app.entries()[0].replacement.clone();
        let buf = render(&app, 120, 30);
        let pos = find_from(&buf, "172.55.0.1", 0).unwrap();
        let on = |s: Style| {
            s.fg == Some(kind_color(Kind::Ipv4)) && s.add_modifier.contains(Modifier::BOLD)
        };
        assert_cells(&buf, pos, "172.55.0.1".len(), on);
        let pos = find_from(&buf, &replacement, 60).unwrap();
        assert_cells(&buf, pos, replacement.len(), on);
        // Text around the finding is plain.
        let (x, y) = find_from(&buf, "server", 0).unwrap();
        assert_eq!(
            style_at(&buf, x, y).fg.unwrap_or(Color::Reset),
            Color::Reset
        );
        assert!(!style_at(&buf, x, y).add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn switched_off_findings_are_dim_and_underlined() {
        let mut app = pasted("server 172.55.0.1 is down");
        focus_findings(&mut app);
        app.update(Action::ToggleEntry);
        let buf = render(&app, 120, 30);
        let off = |s: Style| {
            s.add_modifier.contains(Modifier::DIM)
                && s.add_modifier.contains(Modifier::UNDERLINED)
                && !s.add_modifier.contains(Modifier::BOLD)
        };
        for min_x in [0, 60] {
            let pos = find_from(&buf, "172.55.0.1", min_x).unwrap();
            assert_cells(&buf, pos, "172.55.0.1".len(), off);
        }
    }

    #[test]
    fn findings_panel_shows_entries() {
        let mut app = pasted("a 172.55.0.1 b 198.51.100.7 c 172.55.0.1");
        focus_findings(&mut app);
        app.update(Action::ScrollDown);
        app.update(Action::ToggleEntry);
        let e = app.entries().to_vec();
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "Findings").is_some());
        let (_, y1) = find(&buf, "[x] IPv4").unwrap();
        let (_, y2) = find(&buf, "[ ] IPv4").unwrap();
        assert_eq!(y2, y1 + 1);
        let r1 = row(&buf, y1);
        assert!(
            r1.contains(&format!("172.55.0.1 → {}", e[0].replacement)),
            "{r1}"
        );
        assert!(r1.contains("×2"), "{r1}");
        let r2 = row(&buf, y2);
        assert!(r2.contains(&format!("198.51.100.7 → {}", e[1].replacement)));
        assert!(r2.contains("×1"));
        let (x, _) = find(&buf, "IPv4").unwrap();
        assert_eq!(style_at(&buf, x, y1).fg, Some(kind_color(Kind::Ipv4)));
        assert!(find(&buf, "1 replaced · 1 kept").is_some());
        // Panel height: two entries and the border, below the panes.
        let (_, ty) = find(&buf, "Findings").unwrap();
        assert_eq!(y1, ty + 1);
        assert_eq!(find(&buf, "Obfuscated").unwrap().1, 0);
    }

    #[test]
    fn findings_panel_is_absent_without_findings() {
        let app = pasted("nothing to see");
        let buf = render(&app, 120, 30);
        assert!(find(&buf, "Findings").is_none());
        let (_, empty) = (0, render(&crate::tui::app::tests::app().0, 120, 30));
        assert!(find(&empty, "Findings").is_none());
    }

    #[test]
    fn findings_panel_height_is_at_most_a_third() {
        let text: String = (1..=20).map(|i| format!("198.51.100.{i}\n")).collect();
        let app = pasted(&text);
        let buf = render(&app, 120, 30);
        let (_, ty) = find(&buf, "Findings").unwrap();
        assert_eq!(ty, 30 - 1 - 10);
    }

    #[test]
    fn long_values_are_shortened_in_the_row() {
        let mut app = pasted("a 172.55.0.1 b");
        focus_findings(&mut app);
        let buf = render(&app, 40, 30);
        let (_, y) = find(&buf, "[x] IPv4").unwrap();
        let r: Vec<char> = row(&buf, y).chars().collect();
        assert!(r.contains(&'…'), "{r:?}");
        assert_eq!((r[0], r[39]), ('┃', '┃'));
        assert!(r.iter().collect::<String>().contains("×1"));
    }

    #[test]
    fn shorten_helpers() {
        assert_eq!(shorten_middle("abcdef", 6), "abcdef");
        assert_eq!(shorten_middle("abcdefghij", 5), "ab…ij");
        assert_eq!(shorten_middle("abcdefghij", 1), "…");
        assert_eq!(shorten_middle("abc", 0), "");
        assert!(line_width(&shorten_middle("日本語日本語", 7)) <= 7);
        let (a, b) = fit_pair("aaaaaaaaaa", "b", 6);
        assert_eq!((a.as_str(), b.as_str()), ("aa…aa", "b"));
        let (a, b) = fit_pair(&"a".repeat(20), &"b".repeat(20), 11);
        assert_eq!(line_width(&a) + line_width(&b), 11);
    }

    #[test]
    fn every_kind_has_its_own_colour() {
        let colors: std::collections::HashSet<String> = Kind::ALL
            .iter()
            .map(|k| format!("{:?}", kind_color(*k)))
            .collect();
        assert_eq!(colors.len(), Kind::ALL.len());
    }

    #[test]
    fn focused_area_has_highlighted_border() {
        let mut app = pasted("a 172.55.0.1 b");
        let text_focus = render(&app, 120, 30);
        assert_eq!(text_focus[(0, 0)].symbol(), "┏");
        let (_, ty) = find(&text_focus, "Findings").unwrap();
        assert_eq!(text_focus[(0, ty)].symbol(), "┌");
        focus_findings(&mut app);
        let panel_focus = render(&app, 120, 30);
        assert_eq!(panel_focus[(0, 0)].symbol(), "┌");
        assert_eq!(panel_focus[(0, ty)].symbol(), "┏");
    }

    #[test]
    fn page_size_follows_layout() {
        let a = Rect::new(0, 0, 120, 30);
        assert_eq!(page_size(a, 0), 30 - 1 - 2);
        assert_eq!(page_size(a, 2), 30 - 1 - 4 - 2);
        assert_eq!(page_size(Rect::new(0, 0, 80, 30), 0), (30 - 1) / 2 - 2);
    }
}
