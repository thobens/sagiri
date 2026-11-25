//! The X11 backend: a thread that learns from the XFixes extension when a selection gets
//! a new owner, and reports it as `Event::Changed`.
//!
//! The watcher has its own connection and never reads or writes a selection.

use std::sync::mpsc::Sender;
use std::thread;

use anyhow::Context as _;
use sagiri_core::config::SessionConfig;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::xfixes::{self, ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use super::core::Event;
use crate::clipboard::Selection;

/// The XFixes version this client speaks. Selection notifications need version 1.
const XFIXES_VERSION: (u32, u32) = (5, 0);

/// Connects to the X server, asks for owner-change notifications of `CLIPBOARD` and
/// starts the watcher thread. Errors in these steps are returned before the thread runs.
pub fn start(events: Sender<Event>, _settings: &SessionConfig) -> anyhow::Result<()> {
    let (conn, screen) = RustConnection::connect(None).context("cannot connect to the X server")?;
    if conn
        .extension_information(xfixes::X11_EXTENSION_NAME)
        .context("cannot connect to the X server")?
        .is_none()
    {
        anyhow::bail!("the X server has no XFixes extension");
    }
    // QueryVersion must precede every other XFixes request.
    conn.xfixes_query_version(XFIXES_VERSION.0, XFIXES_VERSION.1)
        .context("the X server has no XFixes extension")?
        .reply()
        .context("the X server has no XFixes extension")?;
    let clipboard = conn
        .intern_atom(false, b"CLIPBOARD")
        .context("cannot connect to the X server")?
        .reply()
        .context("cannot connect to the X server")?
        .atom;
    let primary = u32::from(AtomEnum::PRIMARY);
    let root = conn.setup().roots[screen].root;
    conn.xfixes_select_selection_input(root, clipboard, SelectionEventMask::SET_SELECTION_OWNER)
        .context("cannot watch the clipboard")?
        .check()
        .context("cannot watch the clipboard")?;
    thread::Builder::new()
        .name("x11".into())
        .spawn(move || watch(&conn, &events, clipboard, primary))
        .context("cannot start the X11 thread")?;
    Ok(())
}

/// Blocks on the connection and forwards clipboard owner changes until the connection
/// ends or nobody listens any more.
fn watch(conn: &RustConnection, events: &Sender<Event>, clipboard: u32, primary: u32) {
    loop {
        let event = match conn.wait_for_event() {
            Ok(event) => event,
            Err(_) => {
                let _ = events.send(Event::BackendLost);
                return;
            }
        };
        let XEvent::XfixesSelectionNotify(notify) = event else {
            continue;
        };
        if selection_for(notify.selection, clipboard, primary) != Some(Selection::Clipboard) {
            continue;
        }
        if events.send(Event::Changed(Selection::Clipboard)).is_err() {
            return;
        }
    }
}

/// The selection an atom names, if it is one Sagiri knows.
fn selection_for(atom: u32, clipboard: u32, primary: u32) -> Option<Selection> {
    if atom == clipboard {
        Some(Selection::Clipboard)
    } else if atom == primary {
        Some(Selection::Primary)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_event_mapping() {
        let primary = u32::from(AtomEnum::PRIMARY);
        let clipboard = 300;
        assert_eq!(
            selection_for(clipboard, clipboard, primary),
            Some(Selection::Clipboard)
        );
        assert_eq!(
            selection_for(primary, clipboard, primary),
            Some(Selection::Primary)
        );
        assert_eq!(
            selection_for(u32::from(AtomEnum::SECONDARY), clipboard, primary),
            None
        );
        assert_eq!(selection_for(301, clipboard, primary), None);
    }
}
