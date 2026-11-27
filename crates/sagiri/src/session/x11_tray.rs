//! The tray icon on X11: a thread that embeds a small window in the system tray
//! (freedesktop System Tray Protocol, which builds on XEmbed), draws the state of the
//! session into it and reports a click with button 1 as `Event::Toggle`.
//!
//! Without a tray manager the thread waits for one, and it docks again when the manager
//! is replaced. It has its own connection and never reads or writes a selection. When
//! the connection ends the thread ends silently: the watcher reports the lost display.

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::Context as _;
use x11rb::connection::Connection;
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, BackPixmap, ChangeGCAux, ChangeWindowAttributesAux, ClientMessageEvent,
    Colormap, ColormapAlloc, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, Gcontext,
    PropMode, Rectangle, Screen, Visualid, Visualtype, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT};

use super::core::Event;
use super::tray::{self, Rgb, TrayState};

/// The side of the icon window before the tray manager gives it a size.
const INITIAL_SIDE: u16 = 24;

/// The mouse button that toggles the session.
const TOGGLE_BUTTON: u8 = 1;

/// The window name and the instance and class names of the icon window.
const NAME: &[u8] = b"Sagiri";
const CLASS: &[u8] = b"sagiri-session\0Sagiri\0";

/// The atoms the thread needs.
#[derive(Clone, Copy)]
struct Atoms {
    /// `_NET_SYSTEM_TRAY_S<screen>`, owned by the tray manager.
    selection: Atom,
    opcode: Atom,
    visual: Atom,
    manager: Atom,
    xembed_info: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
    /// The type of the message that the waker of `TrayState` sends.
    wake: Atom,
}

impl Atoms {
    fn intern(conn: &RustConnection, screen: usize) -> anyhow::Result<Self> {
        let atom = |name: &str| -> anyhow::Result<Atom> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .context("cannot look up an atom of the tray protocol")?
                .reply()
                .context("cannot look up an atom of the tray protocol")?
                .atom)
        };
        Ok(Self {
            selection: atom(&tray::selection_name(screen))?,
            opcode: atom("_NET_SYSTEM_TRAY_OPCODE")?,
            visual: atom("_NET_SYSTEM_TRAY_VISUAL")?,
            manager: atom("MANAGER")?,
            xembed_info: atom("_XEMBED_INFO")?,
            net_wm_name: atom("_NET_WM_NAME")?,
            utf8_string: atom("UTF8_STRING")?,
            wake: atom("_SAGIRI_TRAY_WAKE")?,
        })
    }
}

/// Connects to the X server and starts the tray thread. From then on a change of `state`
/// redraws the icon. Errors in the set-up are returned before the thread runs; a missing
/// tray manager is not an error.
pub fn start(events: Sender<Event>, state: Arc<TrayState>) -> anyhow::Result<()> {
    let (conn, screen) = RustConnection::connect(None).context("cannot connect to the X server")?;
    let conn = Arc::new(conn);
    let atoms = Atoms::intern(&conn, screen)?;
    let root = conn.setup().roots[screen].root;

    // Never mapped. It exists so that the waker has a window of this connection to send
    // its message to.
    let wake_window = conn
        .generate_id()
        .context("cannot create the tray's helper window")?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        wake_window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .context("cannot create the tray's helper window")?
    .check()
    .context("cannot create the tray's helper window")?;
    // A new tray manager announces itself with a client message to the root window.
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
    )
    .context("cannot watch for a tray manager")?
    .check()
    .context("cannot watch for a tray manager")?;

    let waker_conn = Arc::clone(&conn);
    state.set_waker(move || {
        // With an empty event mask the message goes to the client that created the
        // window, which is the tray thread. Failures mean that the connection is gone.
        let message = ClientMessageEvent::new(32, wake_window, atoms.wake, [0u32; 5]);
        if let Ok(cookie) = waker_conn.send_event(false, wake_window, EventMask::NO_EVENT, message)
        {
            cookie.ignore_error();
        }
        let _ = waker_conn.flush();
    });

    thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            Tray {
                conn: &conn,
                events: &events,
                state: &state,
                atoms,
                screen,
                root,
                dock: None,
            }
            .run();
        })
        .context("cannot start the tray thread")?;
    Ok(())
}

/// The tray thread ends: the connection is gone, or nobody listens for events any more.
struct Stop;

impl From<ConnectionError> for Stop {
    fn from(_: ConnectionError) -> Self {
        Stop
    }
}

/// The icon window in a tray, and what belongs to it.
struct Dock {
    /// The tray manager the window was offered to.
    manager: Window,
    window: Window,
    gc: Gcontext,
    /// A colour map made for the manager's visual. `None` with the root visual.
    colormap: Option<Colormap>,
    width: u16,
    height: u16,
    /// Pixel values of the two icon colours on the window's visual.
    inactive: u32,
    active: u32,
}

/// The state of the tray thread.
struct Tray<'a> {
    conn: &'a RustConnection,
    events: &'a Sender<Event>,
    state: &'a TrayState,
    atoms: Atoms,
    screen: usize,
    root: Window,
    dock: Option<Dock>,
}

impl Tray<'_> {
    fn run(mut self) {
        if self.find_manager(None).is_err() {
            return;
        }
        loop {
            let Ok(event) = self.conn.wait_for_event() else {
                return;
            };
            if self.handle(event).is_err() {
                return;
            }
        }
    }

    fn handle(&mut self, event: XEvent) -> Result<(), Stop> {
        match event {
            XEvent::ClientMessage(message) if message.format == 32 => {
                if message.type_ == self.atoms.wake {
                    self.draw()
                } else if message.type_ == self.atoms.manager
                    && let Some(manager) =
                        tray::announced_manager(message.data.as_data32(), self.atoms.selection)
                {
                    self.dock_with(manager)
                } else {
                    Ok(())
                }
            }
            XEvent::Expose(expose) if self.is_icon(expose.window) && expose.count == 0 => {
                self.draw()
            }
            XEvent::ConfigureNotify(configure) if self.is_icon(configure.window) => {
                let resized = self.dock.as_mut().is_some_and(|dock| {
                    let changed = (dock.width, dock.height) != (configure.width, configure.height);
                    dock.width = configure.width;
                    dock.height = configure.height;
                    changed
                });
                if resized { self.draw() } else { Ok(()) }
            }
            XEvent::ButtonPress(press)
                if self.is_icon(press.event) && press.detail == TOGGLE_BUTTON =>
            {
                self.events.send(Event::Toggle).map_err(|_| Stop)
            }
            // The manager let go of the window without destroying it.
            XEvent::ReparentNotify(reparent)
                if self.is_icon(reparent.window) && reparent.parent == self.root =>
            {
                let left = self.undock()?;
                self.find_manager(left)
            }
            XEvent::DestroyNotify(destroy) => {
                let gone = destroy.window;
                if self.is_icon(gone) || self.dock.as_ref().is_some_and(|d| d.manager == gone) {
                    let left = self.undock()?;
                    self.find_manager(left)
                } else {
                    Ok(())
                }
            }
            // Everything else, including errors of requests on windows that are gone.
            _ => Ok(()),
        }
    }

    fn is_icon(&self, window: Window) -> bool {
        self.dock.as_ref().is_some_and(|dock| dock.window == window)
    }

    /// Docks with the owner of the tray selection, if there is one. `left` is a manager
    /// that just let go of the icon: it is not asked again until it announces itself.
    fn find_manager(&mut self, left: Option<Window>) -> Result<(), Stop> {
        let owner = match self.conn.get_selection_owner(self.atoms.selection)?.reply() {
            Ok(reply) => reply.owner,
            Err(ReplyError::X11Error(_)) => x11rb::NONE,
            Err(ReplyError::ConnectionError(_)) => return Err(Stop),
        };
        if owner == x11rb::NONE || Some(owner) == left {
            return Ok(());
        }
        self.dock_with(owner)
    }

    /// Creates the icon window for `manager` and asks to be embedded. Does nothing if the
    /// icon is already offered to that manager.
    fn dock_with(&mut self, manager: Window) -> Result<(), Stop> {
        if self.dock.as_ref().is_some_and(|d| d.manager == manager) {
            return Ok(());
        }
        self.undock()?;

        let conn = self.conn;
        let screen = &conn.setup().roots[self.screen];
        let (depth, visual) = self.visual_of(manager, screen)?;
        let alpha = tray::alpha_bits(depth, visual.red_mask, visual.green_mask, visual.blue_mask);
        let colormap = if visual.visual_id == screen.root_visual {
            None
        } else {
            let id = conn.generate_id().map_err(stop)?;
            conn.create_colormap(ColormapAlloc::NONE, id, self.root, visual.visual_id)?
                .ignore_error();
            Some(id)
        };
        let colors = colormap.unwrap_or(screen.default_colormap);

        // Override-redirect, so that no window manager takes the window while it is not
        // embedded. A colour map and a border pixel are required with a visual other than
        // the parent's.
        let attributes = CreateWindowAux::new()
            .override_redirect(1)
            .border_pixel(0)
            .colormap(colors)
            .event_mask(
                EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::STRUCTURE_NOTIFY,
            );
        // `ParentRelative` shows the bar behind the icon, but needs the depth of the
        // parent. A 32-bit visual gets a transparent background instead.
        let attributes = if depth == screen.root_depth && depth != 32 {
            attributes.background_pixmap(u32::from(BackPixmap::PARENT_RELATIVE))
        } else {
            attributes.background_pixel(0)
        };
        let window = conn.generate_id().map_err(stop)?;
        conn.create_window(
            depth,
            window,
            self.root,
            0,
            0,
            INITIAL_SIDE,
            INITIAL_SIDE,
            0,
            WindowClass::INPUT_OUTPUT,
            visual.visual_id,
            &attributes,
        )?
        .ignore_error();
        conn.change_property32(
            PropMode::REPLACE,
            window,
            self.atoms.xembed_info,
            self.atoms.xembed_info,
            &tray::XEMBED_INFO,
        )?
        .ignore_error();
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            NAME,
        )?
        .ignore_error();
        conn.change_property8(
            PropMode::REPLACE,
            window,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            NAME,
        )?
        .ignore_error();
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            CLASS,
        )?
        .ignore_error();
        let gc = conn.generate_id().map_err(stop)?;
        conn.create_gc(gc, window, &CreateGCAux::new().graphics_exposures(0))?
            .ignore_error();
        let inactive = self.pixel(colors, tray::INACTIVE)? | alpha;
        let active = self.pixel(colors, tray::ACTIVE)? | alpha;

        // To learn when the manager goes away.
        conn.change_window_attributes(
            manager,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
        )?
        .ignore_error();
        let request =
            ClientMessageEvent::new(32, manager, self.atoms.opcode, tray::dock_request(window));
        conn.send_event(false, manager, EventMask::NO_EVENT, request)?
            .ignore_error();
        conn.flush()?;

        self.dock = Some(Dock {
            manager,
            window,
            gc,
            colormap,
            width: INITIAL_SIDE,
            height: INITIAL_SIDE,
            inactive,
            active,
        });
        Ok(())
    }

    /// The depth and visual for an icon in the tray of `manager`: what the manager names
    /// in `_NET_SYSTEM_TRAY_VISUAL`, or else the root visual.
    fn visual_of<'s>(
        &self,
        manager: Window,
        screen: &'s Screen,
    ) -> Result<(u8, &'s Visualtype), Stop> {
        let wanted = match self
            .conn
            .get_property(false, manager, self.atoms.visual, AtomEnum::VISUALID, 0, 1)?
            .reply()
        {
            Ok(reply) => reply.value32().and_then(|mut values| values.next()),
            Err(ReplyError::X11Error(_)) => None,
            Err(ReplyError::ConnectionError(_)) => return Err(Stop),
        };
        wanted
            .and_then(|id| find_visual(screen, id))
            .or_else(|| find_visual(screen, screen.root_visual))
            // A screen always lists its root visual.
            .ok_or(Stop)
    }

    /// The pixel value of `color` in `colormap`, without opacity bits. All bits set, which
    /// is white on common visuals, if the colour cannot be allocated.
    fn pixel(&self, colormap: Colormap, color: Rgb) -> Result<u32, Stop> {
        let (red, green, blue) = color.channels16();
        match self.conn.alloc_color(colormap, red, green, blue)?.reply() {
            Ok(reply) => Ok(reply.pixel),
            Err(ReplyError::X11Error(_)) => Ok(u32::MAX),
            Err(ReplyError::ConnectionError(_)) => Err(Stop),
        }
    }

    /// Destroys the icon window and what belongs to it. Returns the manager it was
    /// offered to.
    fn undock(&mut self) -> Result<Option<Window>, Stop> {
        let Some(dock) = self.dock.take() else {
            return Ok(None);
        };
        let conn = self.conn;
        // The window and the manager may be gone already; those errors mean nothing.
        conn.destroy_window(dock.window)?.ignore_error();
        conn.free_gc(dock.gc)?.ignore_error();
        if let Some(colormap) = dock.colormap {
            conn.free_colormap(colormap)?.ignore_error();
        }
        conn.change_window_attributes(
            dock.manager,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT),
        )?
        .ignore_error();
        conn.flush()?;
        Ok(Some(dock.manager))
    }

    /// Draws the icon for the current state and window size.
    fn draw(&self) -> Result<(), Stop> {
        let Some(dock) = &self.dock else {
            return Ok(());
        };
        let active = self.state.is_active();
        let icon = tray::icon(dock.width, dock.height, active);
        let bars: Vec<Rectangle> = icon
            .bars
            .iter()
            .map(|bar| Rectangle {
                x: bar.x,
                y: bar.y,
                width: bar.width,
                height: bar.height,
            })
            .collect();
        let conn = self.conn;
        // Width and height 0 mean the whole window.
        conn.clear_area(false, dock.window, 0, 0, 0, 0)?
            .ignore_error();
        if !bars.is_empty() {
            let pixel = if active { dock.active } else { dock.inactive };
            conn.change_gc(dock.gc, &ChangeGCAux::new().foreground(pixel))?
                .ignore_error();
            conn.poly_fill_rectangle(dock.window, dock.gc, &bars)?
                .ignore_error();
        }
        conn.flush()?;
        Ok(())
    }
}

/// For `generate_id`, which fails when the connection is gone or has no ids left.
fn stop(_: ReplyOrIdError) -> Stop {
    Stop
}

/// The visual `id` of `screen` with its depth.
fn find_visual(screen: &Screen, id: Visualid) -> Option<(u8, &Visualtype)> {
    screen.allowed_depths.iter().find_map(|depth| {
        depth
            .visuals
            .iter()
            .find(|visual| visual.visual_id == id)
            .map(|visual| (depth.depth, visual))
    })
}
