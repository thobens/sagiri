//! The X11 backend: a thread that learns from the XFixes extension when a selection gets
//! a new owner, and reports it as `Event::Changed`. If `session.hotkey` is set, the same
//! connection grabs it on the root window and the thread reports presses as
//! `Event::Toggle`.
//!
//! With `primary = "watch"` the thread also watches `PRIMARY`. A new `PRIMARY` owner is
//! reported only once the selection has settled (see `settle`); while one is pending, the
//! thread polls every 50 ms instead of blocking.
//!
//! The watcher has its own connection and never reads or writes a selection.

use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use sagiri_core::config::{PrimaryMode, SessionConfig};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::ReplyError;
use x11rb::protocol::xfixes::{self, ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, GrabMode, KeyButMask, ModMask, Window,
};
use x11rb::protocol::{ErrorKind, Event as XEvent};
use x11rb::rust_connection::RustConnection;

use super::core::Event;
use super::hotkey::{self, AutoRepeat, Debounce, Hotkey};
use super::settle::Settle;
use crate::clipboard::Selection;

/// The XFixes version this client speaks. Selection notifications need version 1.
const XFIXES_VERSION: (u32, u32) = (5, 0);

/// How often the thread looks at the pointer while a `PRIMARY` selection is pending.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Connects to the X server, asks for owner-change notifications of `CLIPBOARD` (and of
/// `PRIMARY` with `primary = "watch"`), grabs the hotkey if one is set, and starts the
/// watcher thread. Errors in these steps are returned before the thread runs.
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
    let watch_primary = settings.primary == PrimaryMode::Watch;
    if watch_primary {
        conn.xfixes_select_selection_input(root, primary, SelectionEventMask::SET_SELECTION_OWNER)
            .context("cannot watch the primary selection")?
            .check()
            .context("cannot watch the primary selection")?;
    }
    let grab = match hotkey::from_config(settings)? {
        Some(hotkey) => Some(grab(&conn, root, hotkey)?),
        None => None,
    };
    thread::Builder::new()
        .name("x11".into())
        .spawn(move || {
            Watcher {
                conn: &conn,
                events: &events,
                root,
                clipboard,
                primary,
                watch_primary,
                grab,
                debounce: Debounce::new(),
                repeat: AutoRepeat::new(),
                settle: Settle::new(),
            }
            .run();
        })
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

/// The state of the watcher thread.
struct Watcher<'a> {
    conn: &'a RustConnection,
    events: &'a Sender<Event>,
    root: Window,
    clipboard: u32,
    primary: u32,
    watch_primary: bool,
    grab: Option<Grab>,
    debounce: Debounce,
    repeat: AutoRepeat,
    settle: Settle,
}

/// Whether the watcher thread goes on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

impl Watcher<'_> {
    /// Forwards clipboard owner changes, settled `PRIMARY` selections and hotkey presses
    /// until the connection ends or nobody listens any more. Blocks on the connection
    /// while no `PRIMARY` selection is pending.
    fn run(mut self) {
        loop {
            let flow = if self.settle.is_pending() {
                match self.conn.poll_for_event() {
                    Ok(Some(event)) => self.handle(event),
                    Ok(None) => self.tick(),
                    Err(_) => self.lost(),
                }
            } else {
                match self.conn.wait_for_event() {
                    Ok(event) => self.handle(event),
                    Err(_) => self.lost(),
                }
            };
            if flow == Flow::Stop {
                return;
            }
        }
    }

    fn lost(&self) -> Flow {
        let _ = self.events.send(Event::BackendLost);
        Flow::Stop
    }

    fn send(&self, event: Event) -> Flow {
        if self.events.send(event).is_ok() {
            Flow::Continue
        } else {
            Flow::Stop
        }
    }

    /// Runs when every event that has arrived is handled and a `PRIMARY` selection is
    /// pending: asks for the pointer state, lets `Settle` decide, and waits for the next
    /// round if the selection is still pending.
    fn tick(&mut self) -> Flow {
        let held = match self.conn.query_pointer(self.root) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => is_held(reply.mask),
                // A pointer the server cannot report is not held.
                Err(ReplyError::X11Error(_)) => false,
                Err(ReplyError::ConnectionError(_)) => return self.lost(),
            },
            Err(_) => return self.lost(),
        };
        if self.settle.poll(Instant::now(), held) {
            return self.send(Event::Changed(Selection::Primary));
        }
        thread::sleep(POLL_INTERVAL);
        Flow::Continue
    }

    fn handle(&mut self, event: XEvent) -> Flow {
        match event {
            XEvent::KeyRelease(release) => {
                self.repeat.released(release.detail, release.time);
                Flow::Continue
            }
            XEvent::KeyPress(press) => {
                let pressed = self.grab.as_ref().is_some_and(|g| {
                    hotkey::matches(&g.hotkey, &g.keycodes, press.detail, press.state.into())
                });
                if pressed
                    && !self.repeat.is_repeat(press.detail, press.time)
                    && self.debounce.accept(press.time)
                {
                    self.send(Event::Toggle)
                } else {
                    Flow::Continue
                }
            }
            XEvent::XfixesSelectionNotify(notify) => {
                match selection_for(notify.selection, self.clipboard, self.primary) {
                    Some(Selection::Clipboard) => self.send(Event::Changed(Selection::Clipboard)),
                    Some(Selection::Primary) if self.watch_primary => {
                        self.settle.on_owner_change(Instant::now());
                        Flow::Continue
                    }
                    _ => Flow::Continue,
                }
            }
            _ => Flow::Continue,
        }
    }
}

/// Whether mouse button 1 or Shift is down in a pointer state mask.
fn is_held(mask: KeyButMask) -> bool {
    mask.intersects(KeyButMask::BUTTON1 | KeyButMask::SHIFT)
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

    #[test]
    fn held_means_button_one_or_shift() {
        assert!(!is_held(KeyButMask::from(0u16)));
        assert!(is_held(KeyButMask::BUTTON1));
        assert!(is_held(KeyButMask::SHIFT));
        assert!(is_held(KeyButMask::BUTTON1 | KeyButMask::MOD2));
        assert!(!is_held(
            KeyButMask::BUTTON3 | KeyButMask::CONTROL | KeyButMask::LOCK
        ));
    }
}
