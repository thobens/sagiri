//! The X11 backend: a thread that learns from the XFixes extension when a selection gets
//! a new owner, and reports it as `Event::Changed`. If `session.hotkey` is set, the same
//! connection grabs it on the root window and the thread reports presses as
//! `Event::Toggle`.
//!
//! The watcher has its own connection and never reads or writes a selection.

use std::sync::mpsc::Sender;
use std::thread;

use anyhow::Context as _;
use sagiri_core::config::SessionConfig;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::ReplyError;
use x11rb::protocol::xfixes::{self, ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, GrabMode, ModMask, Window};
use x11rb::protocol::{ErrorKind, Event as XEvent};
use x11rb::rust_connection::RustConnection;

use super::core::Event;
use super::hotkey::{self, AutoRepeat, Debounce, Hotkey};
use crate::clipboard::Selection;

/// The XFixes version this client speaks. Selection notifications need version 1.
const XFIXES_VERSION: (u32, u32) = (5, 0);

/// Connects to the X server, asks for owner-change notifications of `CLIPBOARD` and
/// grabs the hotkey if one is set, and starts the watcher thread. Errors in these steps
/// are returned before the thread runs.
pub fn start(events: Sender<Event>, settings: &SessionConfig) -> anyhow::Result<()> {
    let (conn, screen) = RustConnection::connect(None).context("cannot connect to the X server")?;
    if conn
        .extension_information(xfixes::X11_EXTENSION_NAME)
        .context("cannot query the X server for the XFixes extension")?
        .is_none()
    {
        anyhow::bail!("the X server has no XFixes extension");
    }
    // QueryVersion must precede every other XFixes request.
    conn.xfixes_query_version(XFIXES_VERSION.0, XFIXES_VERSION.1)
        .context("cannot negotiate the XFixes version")?
        .reply()
        .context("cannot negotiate the XFixes version")?;
    let clipboard = conn
        .intern_atom(false, b"CLIPBOARD")
        .context("cannot look up the CLIPBOARD atom")?
        .reply()
        .context("cannot look up the CLIPBOARD atom")?
        .atom;
    let primary = u32::from(AtomEnum::PRIMARY);
    let root = conn.setup().roots[screen].root;
    conn.xfixes_select_selection_input(root, clipboard, SelectionEventMask::SET_SELECTION_OWNER)
        .context("cannot watch the clipboard")?
        .check()
        .context("cannot watch the clipboard")?;
    let grab = match hotkey::from_config(settings)? {
        Some(hotkey) => Some(grab(&conn, root, hotkey)?),
        None => None,
    };
    thread::Builder::new()
        .name("x11".into())
        .spawn(move || watch(&conn, &events, clipboard, primary, grab))
        .context("cannot start the X11 thread")?;
    Ok(())
}

/// A grabbed hotkey and the keycodes it was grabbed on.
struct Grab {
    hotkey: Hotkey,
    keycodes: Vec<u8>,
}

/// Grabs `hotkey` on `root`, once per keycode that carries its keysym and per lock
/// variant of its modifiers. Uses checked requests, so that a combination another
/// program holds is reported here.
fn grab(conn: &RustConnection, root: Window, hotkey: Hotkey) -> anyhow::Result<Grab> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let count = max.saturating_sub(min).saturating_add(1);
    let mapping = conn
        .get_keyboard_mapping(min, count)
        .context("cannot read the keyboard mapping")?
        .reply()
        .context("cannot read the keyboard mapping")?;
    let keycodes = hotkey::keycodes_for(
        hotkey.keysym,
        min,
        mapping.keysyms_per_keycode,
        &mapping.keysyms,
    );
    if keycodes.is_empty() {
        anyhow::bail!("session.hotkey names a key that the keyboard layout does not have");
    }
    for &keycode in &keycodes {
        for mask in hotkey::grab_masks(&hotkey) {
            let result = conn
                .grab_key(
                    false,
                    root,
                    ModMask::from(mask),
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .context("cannot grab session.hotkey")?
                .check();
            match result {
                Ok(()) => {}
                Err(ReplyError::X11Error(e)) if e.error_kind == ErrorKind::Access => {
                    anyhow::bail!("session.hotkey is already taken by another program")
                }
                Err(e) => return Err(e).context("cannot grab session.hotkey"),
            }
        }
    }
    Ok(Grab { hotkey, keycodes })
}

/// Blocks on the connection and forwards clipboard owner changes and hotkey presses
/// until the connection ends or nobody listens any more.
fn watch(
    conn: &RustConnection,
    events: &Sender<Event>,
    clipboard: u32,
    primary: u32,
    grab: Option<Grab>,
) {
    let mut debounce = Debounce::new();
    let mut repeat = AutoRepeat::new();
    loop {
        let event = match conn.wait_for_event() {
            Ok(event) => event,
            Err(_) => {
                let _ = events.send(Event::BackendLost);
                return;
            }
        };
        match event {
            XEvent::KeyRelease(release) => {
                repeat.released(release.detail, release.time);
                continue;
            }
            XEvent::KeyPress(press) => {
                let pressed = grab.as_ref().is_some_and(|g| {
                    hotkey::matches(&g.hotkey, &g.keycodes, press.detail, press.state.into())
                });
                if pressed
                    && !repeat.is_repeat(press.detail, press.time)
                    && debounce.accept(press.time)
                    && events.send(Event::Toggle).is_err()
                {
                    return;
                }
                continue;
            }
            _ => {}
        }
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
