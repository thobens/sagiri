//! The terminal UI: paste, obfuscate, copy.

pub mod app;
pub mod clipboard;
pub mod ui;

use std::io::stdout;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context as _;
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use sagiri_core::engine::Engine;

use app::{App, action_for};
use clipboard::SystemClipboard;

static RESTORED: AtomicBool = AtomicBool::new(false);

/// Puts the terminal back to normal. Runs at most once: leaving the alternate screen twice
/// can make some terminals restore a saved cursor position a second time, which would move
/// the cursor above a message printed in between (the panic report).
fn restore_terminal() {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return;
    }
    // Errors cannot be handled usefully while tearing down.
    let _ = execute!(stdout(), DisableBracketedPaste);
    ratatui::restore();
}

/// Restores the terminal when dropped, on every exit path.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// Restores the terminal first, so the message stays visible, then reports the location of
/// the panic. The panic message itself is withheld: it could quote text from the buffer
/// (a slicing error does), and pasted text must never be printed.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        restore_terminal();
        match info.location() {
            Some(l) => eprintln!(
                "sagiri: internal error at {}:{} (message withheld)",
                l.file(),
                l.line()
            ),
            None => eprintln!("sagiri: internal error (message withheld)"),
        }
    }));
}

/// Runs the interactive UI until the user quits.
pub fn run(engine: Engine) -> anyhow::Result<()> {
    let mut app = App::new(engine, Box::new(SystemClipboard::new()));
    // The guard comes first so that a failed init (raw mode on, alternate screen not
    // entered) is also cleaned up.
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init().context("cannot start the terminal UI")?;
    install_panic_hook();
    execute!(stdout(), EnableBracketedPaste).context("cannot enable bracketed paste")?;

    while !app.should_quit() {
        let size = terminal.size().context("cannot read the terminal size")?;
        let panes = size.height.saturating_sub(1);
        let stacked = size.width < 100;
        let pane_height = if stacked { panes / 2 } else { panes };
        app.set_page_size(usize::from(pane_height.saturating_sub(2)));
        terminal
            .draw(|frame| ui::draw(&app, frame))
            .context("cannot draw the screen")?;
        let event = event::read().context("cannot read terminal input")?;
        if let Some(action) = action_for(&event) {
            app.update(action);
        }
    }
    Ok(())
}
